//! Browser artifacts for the host-neutral PolkaVM runtime.

/// One immutable browser runtime file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BrowserAsset {
    /// Relative export path.
    pub path: &'static str,
    /// HTTP content type.
    pub content_type: &'static str,
    /// File contents.
    pub bytes: &'static [u8],
    /// Lowercase SHA-256 digest.
    pub sha256: &'static str,
}

/// Runtime package version shared by every asset.
pub const RUNTIME_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Return the complete browser runtime asset set.
pub fn browser_assets() -> &'static [BrowserAsset] {
    &ASSETS
}

const ASSETS: [BrowserAsset; 8] = [
    BrowserAsset {
        path: "pvm-browser-runtime.wasm",
        content_type: "application/wasm",
        bytes: include_bytes!("../assets/pvm-browser-runtime.wasm"),
        sha256: "86d7405fb0fe3484e361c67dc33544947cf89bd0f0eacd088e6408543f641856",
    },
    BrowserAsset {
        path: "pvm-worker.js",
        content_type: "text/javascript",
        bytes: include_bytes!("../assets/pvm-worker.js"),
        sha256: "7445a46405de366ab3cac88c7d053109b72867b1d0c47df5f618cb3d1002bfe3",
    },
    BrowserAsset {
        path: "pvm-gpu-worker.js",
        content_type: "text/javascript",
        bytes: include_bytes!("../assets/pvm-gpu-worker.js"),
        sha256: "86cb899953b303dca45b0a5f2f2409713809e7223b9a9a5b853c9660d152edec",
    },
    BrowserAsset {
        path: "pvm-wasm-translated.js",
        content_type: "text/javascript",
        bytes: include_bytes!("../assets/pvm-wasm-translated.js"),
        sha256: "9cc181e9aa65b9867c20112bb9bf340679adeb50780e837baffd0e53e0be0616",
    },
    BrowserAsset {
        path: "pvm-runtime-core.js",
        content_type: "text/javascript",
        bytes: include_bytes!("../assets/pvm-runtime-core.js"),
        sha256: "ec7df70950bdfe217d73af3c177585022b27b917533c5385d99075b13f5d2763",
    },
    BrowserAsset {
        path: "pvm-wasm-worker-entry.js",
        content_type: "text/javascript",
        bytes: include_bytes!("../assets/pvm-wasm-worker-entry.js"),
        sha256: "9c929f5d5c64a1b75e7e48485d7c3944ed6838112177ea778827a2c407c2d820",
    },
    BrowserAsset {
        path: "pvm-computer.js",
        content_type: "text/javascript",
        bytes: include_bytes!("../assets/pvm-computer.js"),
        sha256: "55a77833038d07dbaebba9369eead718c3a487c3cff297af4689a2afd551b976",
    },
    BrowserAsset {
        path: "SHA256SUMS",
        content_type: "text/plain",
        bytes: include_bytes!("../assets/SHA256SUMS"),
        sha256: "e66b05bf6b9a4b18ac5ab0291a28430af9797ef444b1ef98732b5e7a48e2d638",
    },
];

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use sha2::{Digest, Sha256};

    use super::*;

    #[test]
    fn embedded_assets_match_their_recorded_digests() {
        let mut paths = HashSet::new();
        for asset in browser_assets() {
            assert!(paths.insert(asset.path), "duplicate asset {}", asset.path);
            let digest = Sha256::digest(asset.bytes)
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>();
            assert_eq!(digest, asset.sha256, "digest mismatch for {}", asset.path);
        }
    }
}
