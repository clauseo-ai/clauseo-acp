//! Shared-hosting domains: one name serves many unrelated sites, so allowing a host under one
//! may allow a stranger's site. This list is a warning, not a refusal: a refused host on or
//! under an entry gets `shared_hosting: true` on its denial, and the card warns more strongly
//! (R18, words through QU.8). A firm wildcard on one is refused
//! ([`Allowlist::firm_suffix`](crate::Allowlist::firm_suffix)).

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
