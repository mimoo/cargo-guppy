(function() {var implementors = {};
implementors["ascii"] = [{"text":"impl ToOwned for AsciiStr","synthetic":false,"types":[]}];
implementors["bstr"] = [{"text":"impl ToOwned for BStr","synthetic":false,"types":[]}];
implementors["openssl"] = [{"text":"impl&lt;T&gt; ToOwned for DsaRef&lt;T&gt;","synthetic":false,"types":[]},{"text":"impl&lt;T&gt; ToOwned for EcKeyRef&lt;T&gt;","synthetic":false,"types":[]},{"text":"impl&lt;T&gt; ToOwned for PKeyRef&lt;T&gt;","synthetic":false,"types":[]},{"text":"impl&lt;T&gt; ToOwned for RsaRef&lt;T&gt;","synthetic":false,"types":[]},{"text":"impl ToOwned for SslSessionRef","synthetic":false,"types":[]},{"text":"impl ToOwned for X509Ref","synthetic":false,"types":[]}];
implementors["serde_json"] = [{"text":"impl ToOwned for RawValue","synthetic":false,"types":[]}];
if (window.register_implementors) {window.register_implementors(implementors);} else {window.pending_implementors = implementors;}})()