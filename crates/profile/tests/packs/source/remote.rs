//! The link and skills.sh name table.

use clauseo_profile::packs::source::{
    FromKind, GitHub, Remote, SourceError, parse_link, parse_skills_sh,
};

fn github(owner: &str, repo: &str, git_ref: Option<&str>, path: Option<&str>) -> Remote {
    Remote::GitHub(GitHub {
        owner: owner.to_owned(),
        repo: repo.to_owned(),
        git_ref: git_ref.map(str::to_owned),
        path: path.map(str::to_owned),
        skill: None,
        via_skills_sh: false,
    })
}

fn skills_sh(owner: &str, repo: &str, git_ref: Option<&str>, skill: Option<&str>) -> Remote {
    Remote::GitHub(GitHub {
        owner: owner.to_owned(),
        repo: repo.to_owned(),
        git_ref: git_ref.map(str::to_owned),
        path: None,
        skill: skill.map(str::to_owned),
        via_skills_sh: true,
    })
}

#[test]
fn links_parse() {
    let table = [
        (
            "https://github.com/anthropics/skills",
            github("anthropics", "skills", None, None),
        ),
        (
            "https://github.com/anthropics/skills/",
            github("anthropics", "skills", None, None),
        ),
        (
            "https://github.com/anthropics/skills.git",
            github("anthropics", "skills", None, None),
        ),
        (
            "https://github.com/anthropics/skills/tree/main",
            github("anthropics", "skills", Some("main"), None),
        ),
        (
            "https://github.com/anthropics/skills/tree/v1.2/skills/pdf",
            github("anthropics", "skills", Some("v1.2"), Some("skills/pdf")),
        ),
        (
            "https://github.com/o/r/blob/main/skills/pdf/SKILL.md",
            github("o", "r", Some("main"), Some("skills/pdf")),
        ),
        (
            "https://skills.sh/anthropics/skills/pdf",
            skills_sh("anthropics", "skills", None, Some("pdf")),
        ),
        (
            "https://example.org/files/kit.ZIP?x=1",
            Remote::Zip {
                url: "https://example.org/files/kit.ZIP?x=1".to_owned(),
            },
        ),
    ];
    for (text, want) in table {
        assert_eq!(parse_link(text), Ok(want), "{text}");
    }
}

#[test]
fn links_refused() {
    for text in [
        "http://github.com/o/r",
        "https://github.com:8443/o/r",
        "https://user:pw@github.com/o/r",
        "https://github.com/o",
        "https://github.com/o/r/issues/1",
        "https://github.com/o/r/tree/main/a%20b",
        "https://github.com/-o/r",
        "https://example.org/page.html",
        "ftp://example.org/x.zip",
        "file:///tmp/x.zip",
        "not a link",
    ] {
        assert_eq!(parse_link(text), Err(SourceError::BadSource), "{text}");
    }
}

#[test]
fn skills_sh_names_parse() {
    let table = [
        (
            "anthropics/skills",
            skills_sh("anthropics", "skills", None, None),
        ),
        (
            "anthropics/skills/pdf",
            skills_sh("anthropics", "skills", None, Some("pdf")),
        ),
        (
            "anthropics/skills@v2",
            skills_sh("anthropics", "skills", Some("v2"), None),
        ),
        (
            " o/r@abc123/pdf ",
            skills_sh("o", "r", Some("abc123"), Some("pdf")),
        ),
    ];
    for (text, want) in table {
        assert_eq!(parse_skills_sh(text), Ok(want), "{text}");
    }
    for text in [
        "",
        "o",
        "o/r/s/t",
        "o/../x",
        "../r",
        "o/r/.hidden",
        "o/r@",
        "o/r@-x",
        "o/r@a..b",
    ] {
        assert_eq!(parse_skills_sh(text), Err(SourceError::BadSource), "{text}");
    }
}

#[test]
fn archive_links_and_origins() {
    let Ok(Remote::GitHub(repo)) = parse_skills_sh("anthropics/skills/pdf") else {
        panic!("parses");
    };
    assert_eq!(
        repo.archive_url(),
        "https://codeload.github.com/anthropics/skills/zip/HEAD"
    );
    let origin = repo.origin();
    assert_eq!(origin.kind, FromKind::SkillsSh);
    assert_eq!(origin.text, "anthropics/skills/pdf");
    assert_eq!(
        origin.source_url.as_deref(),
        Some("https://skills.sh/anthropics/skills/pdf")
    );

    let Ok(remote) = parse_link("https://github.com/o/r/tree/v1/skills/pdf") else {
        panic!("parses");
    };
    assert_eq!(
        remote.download_url(),
        "https://codeload.github.com/o/r/zip/v1"
    );
    let origin = remote.origin();
    assert_eq!(origin.kind, FromKind::GitHub);
    assert_eq!(origin.text, "o/r/skills/pdf");
    assert_eq!(
        origin.source_url.as_deref(),
        Some("https://github.com/o/r/tree/v1/skills/pdf")
    );

    let Ok(zip) = parse_link("https://example.org/kit.zip") else {
        panic!("parses");
    };
    assert_eq!(zip.origin().kind, FromKind::Url);
}

/// A `.zip` link's query and fragment (a signed URL's token) stay out of what is shown and
/// recorded; the download keeps the whole link.
#[test]
fn a_zip_links_query_is_not_shown_or_recorded() {
    let link = "https://files.example.org/kit.zip?X-Amz-Signature=secret&t=1#part";
    let Ok(zip) = parse_link(link) else {
        panic!("parses");
    };
    let origin = zip.origin();
    assert_eq!(origin.text, "https://files.example.org/kit.zip");
    assert_eq!(
        origin.source_url.as_deref(),
        Some("https://files.example.org/kit.zip")
    );
    assert!(zip.download_url().contains("X-Amz-Signature=secret"));
}
