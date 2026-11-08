//! The sha256 of each cargo tool's prebuilt binary, per OS and arch: `Tool::bin_sha256` in
//! `TOOLS` (setup.rs). They change with the versions there.
//!
//! Each `bin_sha256` is the sha256 of the binary taiki-e/install-action installs for that OS and
//! arch, at the install-action commit .github/actions/install-pinned-tools/action.yml pins
//! (1e29709c0ec63babbe8f266397a465f29a825f91, v2.87.31). To derive one: pick the archive as its
//! main.sh does (Linux: the musl build, else gnu, and gnu for cargo-nextest; macOS and Windows on
//! arm64: the `x86_64` build when there is no arm64 one, as for cargo-nextest's universal macOS
//! build), download it from the URL in install-action's manifests/<tool>.json at that commit, check
//! it against the manifest's `hash`, unpack it and hash the binary. cargo-mutants has no Linux
//! arm64 build there, so it has no pin there and is never cached on it.

use super::AssetSha256;

pub(super) const CARGO_NEXTEST: &[AssetSha256] = &[
    AssetSha256 {
        os: "macos",
        arch: "aarch64",
        sha256: "aac4a0d9109efcfb4d83ec729c6e3204d5c15bf16ff554b5ee4476f6c5f60b37",
    },
    AssetSha256 {
        os: "macos",
        arch: "x86_64",
        sha256: "aac4a0d9109efcfb4d83ec729c6e3204d5c15bf16ff554b5ee4476f6c5f60b37",
    },
    AssetSha256 {
        os: "linux",
        arch: "aarch64",
        sha256: "546f3e545dd4a9a42201ff542ac25af33253db37809a4c90dd26dbfaed6da4e4",
    },
    AssetSha256 {
        os: "linux",
        arch: "x86_64",
        sha256: "53470cfc9b5cbcd96780db2d348700d76af962467503791f1f8a8824d8967e5c",
    },
    AssetSha256 {
        os: "windows",
        arch: "x86_64",
        sha256: "d9dd12a3d5dd67d5b8ca9315fa9f3c2384d1d47f4e324039ba634af36e2862d8",
    },
    AssetSha256 {
        os: "windows",
        arch: "aarch64",
        sha256: "b04157f7b5905fe6127a0a587e97952f3b2cfd35e8d4ad957a0411deb25f90b1",
    },
];

pub(super) const CARGO_DENY: &[AssetSha256] = &[
    AssetSha256 {
        os: "macos",
        arch: "aarch64",
        sha256: "5f65c07c459c9514f0c97cc2e2fb6b120daef2d95aee31062cab4816cf027eb1",
    },
    AssetSha256 {
        os: "macos",
        arch: "x86_64",
        sha256: "325ae013000059dfa398557f7bc35659e1d73af6b0c39955dff633de164f9c1e",
    },
    AssetSha256 {
        os: "linux",
        arch: "aarch64",
        sha256: "21e797104e371854dde7f05f52d834e70312749d361c168429ced996f2194462",
    },
    AssetSha256 {
        os: "linux",
        arch: "x86_64",
        sha256: "b329e25933d01c36dd7c47d84ea5716694f9b7caf53a5003d45674703a8ed54a",
    },
    AssetSha256 {
        os: "windows",
        arch: "x86_64",
        sha256: "f7292fab58c706638c999e64c4ba82e5128ae628130ba55e3266a768ee431fbf",
    },
    AssetSha256 {
        os: "windows",
        arch: "aarch64",
        sha256: "f7292fab58c706638c999e64c4ba82e5128ae628130ba55e3266a768ee431fbf",
    },
];

pub(super) const CARGO_SHEAR: &[AssetSha256] = &[
    AssetSha256 {
        os: "macos",
        arch: "aarch64",
        sha256: "34edccc283039ed8c2351c72c45c21a13ed65cf1fdde915f689ac97b060150c5",
    },
    AssetSha256 {
        os: "macos",
        arch: "x86_64",
        sha256: "ec245d620892855e0ccccbd16bf5a4abedbe752c1375b474b9540deac99b1de7",
    },
    AssetSha256 {
        os: "linux",
        arch: "aarch64",
        sha256: "98a02bef25f9f098b124098ae4a38bc591a1748734bb425ed5e1d58dd7f1bbf4",
    },
    AssetSha256 {
        os: "linux",
        arch: "x86_64",
        sha256: "66c6f91695d654846becf97df2c1083545003e28f79389e93b44e8cb0d15ac70",
    },
    AssetSha256 {
        os: "windows",
        arch: "x86_64",
        sha256: "b4084260f8b2ffe65c5354f705432497a55690bda119654bc35479107c22b779",
    },
    AssetSha256 {
        os: "windows",
        arch: "aarch64",
        sha256: "47efe03996830c3ff17c9e42bf2d21470bd76cb698208c1faabac3322f371411",
    },
];

pub(super) const TYPOS_CLI: &[AssetSha256] = &[
    AssetSha256 {
        os: "macos",
        arch: "aarch64",
        sha256: "4bbd6348235106ad321de4481e1201f2641189d880f9fa15a2a5da3849f27b12",
    },
    AssetSha256 {
        os: "macos",
        arch: "x86_64",
        sha256: "36bca215941754ae5cdec0d2618ff41a6c134e2119390aa9f797d0dbc141fb1c",
    },
    AssetSha256 {
        os: "linux",
        arch: "aarch64",
        sha256: "81a1d05bd031b46373d09873e6dc40fb53d98ed1ae2a78251217ac08f39c6a74",
    },
    AssetSha256 {
        os: "linux",
        arch: "x86_64",
        sha256: "4f9c222875860b0fb3651cb76c8b10b0bd6513d486db7608e8f5ace7fba7757a",
    },
    AssetSha256 {
        os: "windows",
        arch: "x86_64",
        sha256: "88aa2ead06d7ea7f45de968b5971d55dfabe84ad2c39fe6749f6e8e05377839a",
    },
    AssetSha256 {
        os: "windows",
        arch: "aarch64",
        sha256: "88aa2ead06d7ea7f45de968b5971d55dfabe84ad2c39fe6749f6e8e05377839a",
    },
];

pub(super) const TAPLO_CLI: &[AssetSha256] = &[
    AssetSha256 {
        os: "macos",
        arch: "aarch64",
        sha256: "13cd257c1cadb003b40daf82b3fb1451e012e2463b760bdd33df07a07970c604",
    },
    AssetSha256 {
        os: "macos",
        arch: "x86_64",
        sha256: "9fd7a2872ea154df61a2c7e9ca69fc19ac08e29f2e2dc2f866e299bdc789c1a1",
    },
    AssetSha256 {
        os: "linux",
        arch: "aarch64",
        sha256: "82df9d765856d0d94d2147cc0912016e4a2bfb96cbe947347b7cc04c7f4431ba",
    },
    AssetSha256 {
        os: "linux",
        arch: "x86_64",
        sha256: "dad2faf6377d2daa4f4fabf459fe7ccfb98a5448f0d4bca8270ca9acb0409bfe",
    },
    AssetSha256 {
        os: "windows",
        arch: "x86_64",
        sha256: "12660d24becf05a78b39133112e91d1bd319b22b8e49660448c222a6858a6827",
    },
    AssetSha256 {
        os: "windows",
        arch: "aarch64",
        sha256: "467616481c9c9b96b5a80385c6ec5bfee03b214ab62538d9ed1175cb286702de",
    },
];

pub(super) const CARGO_MUTANTS: &[AssetSha256] = &[
    AssetSha256 {
        os: "macos",
        arch: "aarch64",
        sha256: "7675dc9fcc627488cfd9f23eafafbd5609a7a2870f7adc6b8b65f22cf1e89efb",
    },
    AssetSha256 {
        os: "macos",
        arch: "x86_64",
        sha256: "7675dc9fcc627488cfd9f23eafafbd5609a7a2870f7adc6b8b65f22cf1e89efb",
    },
    AssetSha256 {
        os: "linux",
        arch: "x86_64",
        sha256: "f985f265ee3ea3e453aa98b04c52134953911f692f8ce8abf137f3873202a2d0",
    },
    AssetSha256 {
        os: "windows",
        arch: "x86_64",
        sha256: "03abe0bd3d3b5e000311a75d294fedbb22e8cb2c863814775c782bf04a0fce4b",
    },
    AssetSha256 {
        os: "windows",
        arch: "aarch64",
        sha256: "03abe0bd3d3b5e000311a75d294fedbb22e8cb2c863814775c782bf04a0fce4b",
    },
];
