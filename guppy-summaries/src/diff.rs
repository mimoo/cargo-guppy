// Copyright (c) The cargo-guppy Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

use crate::{
    PackageInfo, PackageMap, PackageStatus, SummaryId, SummarySource, SummaryWithMetadata,
};
use diffus::{edit, Diffable};
use semver::Version;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::{fmt, mem};

/// A diff of two summaries.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SummaryDiff<'a> {
    /// Diff of target packages.
    pub target_packages: PackageDiff<'a>,

    /// Diff of host packages.
    pub host_packages: PackageDiff<'a>,
}

impl<'a> SummaryDiff<'a> {
    /// Computes a diff between two summaries.
    pub fn new<M1, M2>(old: &'a SummaryWithMetadata<M1>, new: &'a SummaryWithMetadata<M2>) -> Self {
        Self {
            target_packages: PackageDiff::new(&old.target_packages, &new.target_packages),
            host_packages: PackageDiff::new(&old.host_packages, &new.host_packages),
        }
    }

    /// Returns true if there are any changes in this diff.
    pub fn is_changed(&self) -> bool {
        !self.is_unchanged()
    }

    /// Returns true if there are no changes in this diff.
    pub fn is_unchanged(&self) -> bool {
        self.target_packages.is_unchanged() && self.host_packages.is_unchanged()
    }
}

impl<'a> fmt::Display for SummaryDiff<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if !self.target_packages.is_unchanged() {
            writeln!(f, "target packages:\n{}", self.target_packages)?;
        }
        if !self.host_packages.is_unchanged() {
            writeln!(f, "host packages:\n{}", self.host_packages)?;
        }

        Ok(())
    }
}

/// Type alias for list entries in the `PackageDiff::unchanged` map.
pub type UnchangedInfo<'a> = (&'a Version, &'a SummarySource, &'a PackageInfo);

/// A diff from a particular section of a summary.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PackageDiff<'a> {
    /// Changed packages.
    pub changed: BTreeMap<&'a SummaryId, SummaryDiffStatus<'a>>,

    /// Unchanged packages, keyed by name.
    pub unchanged: BTreeMap<&'a str, Vec<UnchangedInfo<'a>>>,
}

impl<'a> PackageDiff<'a> {
    /// Constructs a new `PackageDiff` from a pair of `PackageMap` instances.
    pub fn new(old: &'a PackageMap, new: &'a PackageMap) -> Self {
        let mut changed = BTreeMap::new();
        let mut unchanged = BTreeMap::new();

        let mut add_unchanged = |summary_id: &'a SummaryId, info: &'a PackageInfo| {
            unchanged
                .entry(summary_id.name.as_str())
                .or_insert_with(Vec::new)
                .push((&summary_id.version, &summary_id.source, info));
        };

        match (*old).diff(new) {
            edit::Edit::Copy(_) => {
                // Add all elements to unchanged.
                for (summary_id, info) in new {
                    add_unchanged(summary_id, info);
                }
            }
            edit::Edit::Change(diff) => {
                for (summary_id, diff) in diff {
                    match diff {
                        edit::map::Edit::Copy(info) => {
                            // No changes.
                            add_unchanged(summary_id, info);
                        }
                        edit::map::Edit::Insert(info) => {
                            // New package.
                            let status = SummaryDiffStatus::Added { info };
                            changed.insert(summary_id, status);
                        }
                        edit::map::Edit::Remove(old_info) => {
                            // Removed package.
                            let status = SummaryDiffStatus::Removed { old_info };
                            changed.insert(summary_id, status);
                        }
                        edit::map::Edit::Change((old_info, new_info)) => {
                            // The feature set or status changed.
                            let status =
                                SummaryDiffStatus::make_changed(None, None, old_info, new_info);
                            changed.insert(summary_id, status);
                        }
                    }
                }
            }
        }

        // Combine lone inserts and removes into changes.
        Self::combine_insert_remove(&mut changed);

        Self { changed, unchanged }
    }

    /// Returns true if there are no changes in this diff.
    pub fn is_unchanged(&self) -> bool {
        self.changed.is_empty()
    }

    // ---
    // Helper methods
    // ---

    fn combine_insert_remove(changed: &mut BTreeMap<&'a SummaryId, SummaryDiffStatus<'a>>) {
        let mut combine_statuses = HashMap::with_capacity(changed.len());

        for (summary_id, status) in &*changed {
            let entry = combine_statuses
                .entry(summary_id.name.as_str())
                .or_insert_with(|| CombineStatus::None);
            match status {
                SummaryDiffStatus::Added { .. } => entry.record_added(summary_id),
                SummaryDiffStatus::Removed { .. } => entry.record_removed(summary_id),
                SummaryDiffStatus::Changed { .. } => entry.record_changed(),
            }
        }

        for status in combine_statuses.values() {
            if let CombineStatus::Combine { added, removed } = status {
                let removed_status = changed
                    .remove(removed)
                    .expect("removed ID should be present");

                let old_info = match removed_status {
                    SummaryDiffStatus::Removed { old_info } => old_info,
                    other => panic!("expected Removed, found {:?}", other),
                };

                let added_status = changed.get_mut(added).expect("added ID should be present");
                let new_info = match &*added_status {
                    SummaryDiffStatus::Added { info } => *info,
                    other => panic!("expected Added, found {:?}", other),
                };

                let old_version = if added.version != removed.version {
                    Some(&removed.version)
                } else {
                    None
                };
                let old_source = if added.source != removed.source {
                    Some(&removed.source)
                } else {
                    None
                };

                mem::replace(
                    added_status,
                    SummaryDiffStatus::make_changed(old_version, old_source, old_info, new_info),
                );
            }
        }
    }
}

impl<'a> fmt::Display for PackageDiff<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (summary_id, status) in &self.changed {
            if let Some(new_status) = status.new_status() {
                write!(
                    f,
                    "  {} {} ({}, {})",
                    summary_id.name, summary_id.version, new_status, summary_id.source
                )?;
            } else {
                write!(
                    f,
                    "  {} {} ({})",
                    summary_id.name, summary_id.version, summary_id.source
                )?;
            }

            // Print out other versions if available.
            if let Some(unchanged_list) = self.unchanged.get(summary_id.name.as_str()) {
                write!(f, " (other versions: ")?;
                display_list(f, unchanged_list.iter().map(|(version, _, _)| *version))?;
                write!(f, ")")?;
            }

            writeln!(f)?;

            match status {
                SummaryDiffStatus::Added { info } => {
                    write!(f, "    * added package, features: ")?;
                    display_list(f, &info.features)?;
                    writeln!(f)?;
                }
                SummaryDiffStatus::Removed { old_info } => {
                    write!(
                        f,
                        "    * removed package, old status: {}, old features: ",
                        old_info.status
                    )?;
                    display_list(f, &old_info.features)?;
                    writeln!(f)?;
                }
                SummaryDiffStatus::Changed {
                    old_version,
                    old_source,
                    old_status,
                    // The new status is printed in the package header.
                    new_status: _,
                    added_features,
                    removed_features,
                    unchanged_features,
                } => {
                    if let Some(old_version) = old_version {
                        let change_str = if summary_id.version > **old_version {
                            "upgraded"
                        } else {
                            "DOWNGRADED"
                        };
                        writeln!(f, "    * version {} from {}", change_str, old_version)?;
                    }
                    if let Some(old_source) = old_source {
                        writeln!(f, "    * source changed from {}", old_source)?;
                    }
                    if let Some(old_status) = old_status {
                        writeln!(f, "    * status changed from {}", old_status)?;
                    }
                    if !added_features.is_empty() {
                        write!(f, "    * added features: ")?;
                        display_list(f, added_features.iter().copied())?;
                        writeln!(f)?;
                    }
                    if !removed_features.is_empty() {
                        write!(f, "    * removed features: ")?;
                        display_list(f, removed_features.iter().copied())?;
                        writeln!(f)?;
                    }
                    write!(f, "    * (unchanged features: ")?;
                    display_list(f, unchanged_features.iter().copied())?;
                    writeln!(f, ")")?;
                }
            }
        }

        Ok(())
    }
}

fn display_list<I>(f: &mut fmt::Formatter, items: I) -> fmt::Result
where
    I: IntoIterator,
    I::Item: fmt::Display,
    I::IntoIter: ExactSizeIterator,
{
    let items = items.into_iter();
    let len = items.len();
    for (idx, item) in items.enumerate() {
        write!(f, "{}", item)?;
        // Add a comma for all items except the last one.
        if idx + 1 < len {
            write!(f, ", ")?;
        }
    }

    Ok(())
}

/// The diff status for a particular summary ID and source.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SummaryDiffStatus<'a> {
    /// This package was added.
    Added {
        /// The information for this package.
        info: &'a PackageInfo,
    },

    /// This package was removed.
    Removed {
        /// The information this package used to have.
        old_info: &'a PackageInfo,
    },

    /// Some details about the package changed:
    /// * a feature was added or removed
    /// * the version or source changed.
    Changed {
        /// The old version of this package, if the version changed.
        old_version: Option<&'a Version>,

        /// The old source of this package, if the source changed.
        old_source: Option<&'a SummarySource>,

        /// The old status of this package, if the status changed.
        old_status: Option<PackageStatus>,

        /// The current status of this package.
        new_status: PackageStatus,

        /// The set of features added to the package.
        added_features: BTreeSet<&'a str>,

        /// The set of features removed from the package.
        removed_features: BTreeSet<&'a str>,

        /// The set of features which were enabled both in both the old and new summaries.
        unchanged_features: BTreeSet<&'a str>,
    },
}

impl<'a> SummaryDiffStatus<'a> {
    fn make_changed(
        old_version: Option<&'a Version>,
        old_source: Option<&'a SummarySource>,
        old_info: &'a PackageInfo,
        new_info: &'a PackageInfo,
    ) -> Self {
        let old_status = if old_info.status != new_info.status {
            Some(old_info.status)
        } else {
            None
        };

        match old_info.features.diff(&new_info.features) {
            edit::Edit::Copy(features) => {
                // No diff between the old and new features, so put everything in unchanged.
                let unchanged_features = features.iter().map(|feature| feature.as_str()).collect();
                SummaryDiffStatus::Changed {
                    old_version,
                    old_source,
                    old_status,
                    new_status: new_info.status,
                    added_features: BTreeSet::new(),
                    removed_features: BTreeSet::new(),
                    unchanged_features,
                }
            }
            edit::Edit::Change(diff) => {
                Self::make_changed_diff(old_version, old_source, old_status, new_info.status, diff)
            }
        }
    }

    fn make_changed_diff(
        old_version: Option<&'a Version>,
        old_source: Option<&'a SummarySource>,
        old_status: Option<PackageStatus>,
        new_status: PackageStatus,
        feature_diff: BTreeMap<&'a String, edit::set::Edit<'a, String>>,
    ) -> Self {
        let mut added_features = BTreeSet::new();
        let mut removed_features = BTreeSet::new();
        let mut unchanged_features = BTreeSet::new();

        for (_, diff) in feature_diff {
            match diff {
                edit::set::Edit::Copy(feature) => {
                    unchanged_features.insert(feature.as_str());
                }
                edit::set::Edit::Insert(feature) => {
                    added_features.insert(feature.as_str());
                }
                edit::set::Edit::Remove(feature) => {
                    removed_features.insert(feature.as_str());
                }
            }
        }

        SummaryDiffStatus::Changed {
            old_version,
            old_source,
            old_status,
            new_status,
            added_features,
            removed_features,
            unchanged_features,
        }
    }

    fn new_status(&self) -> Option<PackageStatus> {
        match self {
            SummaryDiffStatus::Added { info } => Some(info.status),
            SummaryDiffStatus::Removed { .. } => None,
            SummaryDiffStatus::Changed { new_status, .. } => Some(*new_status),
        }
    }
}

impl<'a> Diffable<'a> for PackageInfo {
    type Diff = (&'a PackageInfo, &'a PackageInfo);

    fn diff(&'a self, other: &'a Self) -> edit::Edit<'a, Self> {
        if self == other {
            edit::Edit::Copy(self)
        } else {
            edit::Edit::Change((self, other))
        }
    }
}

impl<'a> Diffable<'a> for PackageStatus {
    type Diff = (&'a PackageStatus, &'a PackageStatus);

    fn diff(&'a self, other: &'a Self) -> edit::Edit<'a, Self> {
        if self == other {
            edit::Edit::Copy(self)
        } else {
            edit::Edit::Change((self, other))
        }
    }
}

// Status tracker for combining inserts and removes.
enum CombineStatus<'a> {
    None,
    Added(&'a SummaryId),
    Removed(&'a SummaryId),
    Combine {
        added: &'a SummaryId,
        removed: &'a SummaryId,
    },
    Ignore,
}

impl<'a> CombineStatus<'a> {
    fn record_added(&mut self, summary_id: &'a SummaryId) {
        let new = match self {
            CombineStatus::None => CombineStatus::Added(summary_id),
            CombineStatus::Removed(removed) => CombineStatus::Combine {
                added: summary_id,
                removed,
            },
            _ => CombineStatus::Ignore,
        };

        mem::replace(self, new);
    }

    fn record_removed(&mut self, summary_id: &'a SummaryId) {
        let new = match self {
            CombineStatus::None => CombineStatus::Removed(summary_id),
            CombineStatus::Added(added) => CombineStatus::Combine {
                added,
                removed: summary_id,
            },
            _ => CombineStatus::Ignore,
        };

        mem::replace(self, new);
    }

    fn record_changed(&mut self) {
        // If this package name appears in the changed list at all, don't combine its
        // features.
        mem::replace(self, CombineStatus::Ignore);
    }
}
