//! Shared-hosting domains: one name serves many unrelated sites, so allowing a host under one
//! may allow a stranger's site. This list is a warning, not a refusal: a refused host on or
//! under an entry gets `shared_hosting: true` on its denial, and the card warns more strongly
//! (R18, words through QU.8). A firm wildcard on, under or above one is refused
//! ([`Allowlist::firm_suffix`](crate::Allowlist::firm_suffix)), and so is one on or above a
//! domain of [`PUBLIC_SUFFIX_PARENTS`].

/// A host equal to or under one of these is shared hosting. Lowercase ASCII, no trailing dot.
pub(crate) const SHARED_HOSTS: &[&str] = &[
    // Cloud storage and CDNs.
    "amazonaws.com",
    "s3.amazonaws.com",
    "cloudfront.net",
    "blob.core.windows.net",
    "web.core.windows.net",
    "azureedge.net",
    "azurefd.net",
    "storage.googleapis.com",
    "googleusercontent.com",
    "r2.dev",
    "b-cdn.net",
    "fastly.net",
    "akamaized.net",
    "digitaloceanspaces.com",
    // Code hosting and user content.
    "github.io",
    "githubusercontent.com",
    "raw.githubusercontent.com",
    "gitlab.io",
    "bitbucket.io",
    // App hosting.
    "appspot.com",
    "azurewebsites.net",
    "cloudapp.net",
    "herokuapp.com",
    "herokudns.com",
    "netlify.app",
    "vercel.app",
    "pages.dev",
    "workers.dev",
    "firebaseapp.com",
    "web.app",
    "fly.dev",
    "onrender.com",
    "glitch.me",
    "replit.app",
    "ngrok.io",
    "ngrok-free.app",
    // Site builders and blogs.
    "blogspot.com",
    "wordpress.com",
    "wixsite.com",
    "squarespace.com",
    "weebly.com",
    "tumblr.com",
    "notion.site",
    "sharepoint.com",
];

/// Domains with Public Suffix List rules under them (private section included) that the `psl`
/// crate cannot report, since it cannot list its rules: a firm wildcard on one of these, or on
/// a parent of one, covers strangers' accounts. The review of S16 named these; add to it when
/// another turns up. Lowercase ASCII, no trailing dot.
pub(crate) const PUBLIC_SUFFIX_PARENTS: &[&str] = &[
    // Azure storage, web and database accounts (`blob.core.windows.net` and its siblings).
    "core.windows.net",
    "windows.net",
    // Cloudflare R2 buckets.
    "cloudflarestorage.com",
    // Firebase realtime databases.
    "firebaseio.com",
    // Dropbox shared content.
    "dropboxusercontent.com",
    // Google Cloud Run services.
    "run.app",
    // AWS Lambda function URLs and other per-account AWS hosts.
    "on.aws",
    // Microsoft 365 tenants.
    "onmicrosoft.com",
    // Names that resolve to any address written in them.
    "nip.io",
    "sslip.io",
];
