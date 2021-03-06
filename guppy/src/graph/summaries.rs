// Copyright (c) The cargo-guppy Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Generate build summaries from `CargoSet` instances.
//!
//! Requires the `summaries` feature to be enabled.

use crate::graph::cargo::{CargoOptions, CargoResolverVersion, CargoSet};
use crate::graph::feature::{FeatureQuery, FeatureSet};
use crate::graph::{DependencyDirection, PackageGraph, PackageMetadata, PackageSet, PackageSource};
use crate::Error;
pub use guppy_summaries::*;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use target_spec::summaries::PlatformSummary;

/// A type alias for build summaries generated by `guppy`.
pub type Summary = SummaryWithMetadata<CargoOptionsSummary>;

impl<'g> CargoSet<'g> {
    /// Creates a build summary with the given options.
    ///
    /// Requires the `summaries` feature to be enabled.
    pub fn to_summary(&self, opts: &CargoOptions<'_>) -> Result<Summary, Error> {
        let original_query = self.original_query();
        let metadata = CargoOptionsSummary::new(original_query.graph().package_graph, opts)?;
        let target_features = self.target_features();
        let host_features = self.host_features();

        Ok(Summary {
            metadata: Some(metadata),
            target_packages: target_features
                .to_package_map(original_query, self.target_direct_deps()),
            host_packages: host_features.to_package_map(original_query, self.host_direct_deps()),
        })
    }
}

impl<'g> FeatureSet<'g> {
    /// Creates a `PackageMap` from this `FeatureSet`.
    ///
    /// `original_query` and `direct_deps` are used to assign a PackageStatus.
    fn to_package_map(
        &self,
        original_query: &FeatureQuery<'g>,
        direct_deps: &PackageSet<'g>,
    ) -> PackageMap {
        self.packages_with_features(DependencyDirection::Forward)
            .map(|feature_list| {
                let package = feature_list.package();

                let status = if original_query.starts_from_package_ix(package.package_ix()) {
                    PackageStatus::Initial
                } else if package.in_workspace() {
                    PackageStatus::Workspace
                } else if direct_deps.contains_ix(package.package_ix()) {
                    PackageStatus::Direct
                } else {
                    PackageStatus::Transitive
                };

                let info = PackageInfo {
                    status,
                    features: feature_list
                        .features()
                        .iter()
                        .map(|feature| feature.to_string())
                        .collect(),
                };

                (feature_list.package().to_summary_id(), info)
            })
            .collect()
    }
}

impl<'g> PackageMetadata<'g> {
    /// Converts this metadata to a `SummaryId`.
    fn to_summary_id(&self) -> SummaryId {
        SummaryId {
            name: self.name().to_string(),
            version: self.version().clone(),
            source: self.source().to_summary_source(),
        }
    }
}

/// A summary of Cargo options used to build a `CargoSet`.
///
/// Requires the `summaries` feature to be enabled.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
#[non_exhaustive]
pub struct CargoOptionsSummary {
    /// The version of resolver used.
    pub version: CargoResolverVersion,

    /// Whether dev-dependencies are included.
    pub include_dev: bool,

    /// Whether procedural macros specified in initials are included in the target set.
    pub proc_macros_on_target: bool,

    /// The host platform.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub host_platform: Option<PlatformSummary>,

    /// The target platform.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub target_platform: Option<PlatformSummary>,

    /// The set of packages omitted from computations.
    #[serde(skip_serializing_if = "BTreeSet::is_empty", default)]
    pub omitted_packages: BTreeSet<SummaryId>,
}

impl CargoOptionsSummary {
    /// Creates a new `CargoOptionsSummary` from the given Cargo options.
    pub fn new(graph: &PackageGraph, opts: &CargoOptions<'_>) -> Result<Self, Error> {
        let omitted_summary_ids = opts
            .omitted_packages
            .iter()
            .map(|package_id| {
                let metadata = graph.metadata(package_id).expect("valid package ID");
                metadata.to_summary_id()
            })
            .collect();
        Ok(Self {
            version: opts.version,
            include_dev: opts.include_dev,
            proc_macros_on_target: opts.proc_macros_on_target,
            host_platform: opts
                .host_platform()
                .map(PlatformSummary::new)
                .transpose()
                .map_err(|err| {
                    Error::TargetSpecError("while serializing host platform".to_string(), err)
                })?,
            target_platform: opts
                .target_platform()
                .map(PlatformSummary::new)
                .transpose()
                .map_err(|err| {
                    Error::TargetSpecError("while serializing target platform".to_string(), err)
                })?,
            omitted_packages: omitted_summary_ids,
        })
    }

    /// Creates a new `CargoOptions` from this summary.
    pub fn to_cargo_options<'g>(
        &'g self,
        package_graph: &'g PackageGraph,
    ) -> Result<CargoOptions<'g>, Error> {
        let omitted_packages = self
            .omitted_packages
            .iter()
            .map(|summary_id| match &summary_id.source {
                SummarySource::Workspace { workspace_path } => package_graph
                    .workspace()
                    .member_by_path(workspace_path)
                    .map(|package| package.id()),
                other => unimplemented!(
                    "conversion from non-workspace sources ({:?}) is currently unsupported",
                    other
                ),
            })
            .collect::<Result<Vec<_>, _>>()?;

        Ok(CargoOptions::new()
            .with_version(self.version)
            .with_dev_deps(self.include_dev)
            .with_proc_macros_on_target(self.proc_macros_on_target)
            .with_host_platform(
                self.host_platform
                    .as_ref()
                    .map(|platform| platform.to_platform())
                    .transpose()
                    .map_err(|err| {
                        Error::TargetSpecError("parsing host platform".to_string(), err)
                    })?,
            )
            .with_target_platform(
                self.target_platform
                    .as_ref()
                    .map(|platform| platform.to_platform())
                    .transpose()
                    .map_err(|err| {
                        Error::TargetSpecError("parsing target platform".to_string(), err)
                    })?,
            )
            .with_omitted_packages(omitted_packages))
    }
}

impl<'g> PackageSource<'g> {
    /// Converts a `PackageSource` into a `SummarySource`.
    ///
    /// Requires the `summaries` feature to be enabled.
    pub fn to_summary_source(&self) -> SummarySource {
        match self {
            PackageSource::Workspace(path) => SummarySource::workspace(path),
            PackageSource::Path(path) => SummarySource::path(path),
            PackageSource::External(source) => {
                if *source == PackageSource::CRATES_IO_REGISTRY {
                    SummarySource::crates_io()
                } else {
                    SummarySource::external(*source)
                }
            }
        }
    }
}
