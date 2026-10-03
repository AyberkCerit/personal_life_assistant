//! The download allow-list (FR-MDL-001): only HTTPS to Hugging Face and its CDN.

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PolicyError {
    #[error("only https downloads are allowed")]
    NotHttps,
    #[error("{0} is not an allowed download host")]
    HostNotAllowed(String),
    #[error("not a usable download address: {0}")]
    BadUrl(String),
}

/// Decides whether PLA may connect to a URL. Every redirect is checked too (download.rs).
pub trait UrlPolicy: Send + Sync {
    fn allows(&self, url: &str) -> Result<(), PolicyError>;
}

/// The production rule: HTTPS to huggingface.co or a *.hf.co CDN host.
pub struct HuggingFace;

impl UrlPolicy for HuggingFace {
    fn allows(&self, url: &str) -> Result<(), PolicyError> {
        let (scheme, host) = scheme_and_host(url)?;
        if scheme != "https" {
            return Err(PolicyError::NotHttps);
        }
        if host == "huggingface.co" || host.ends_with(".hf.co") {
            Ok(())
        } else {
            Err(PolicyError::HostNotAllowed(host))
        }
    }
}

/// Lower-case scheme and host of an absolute URL; user info (`a@b`) is refused.
pub fn scheme_and_host(url: &str) -> Result<(String, String), PolicyError> {
    let bad = || PolicyError::BadUrl(url.to_owned());
    let (scheme, rest) = url.split_once("://").ok_or_else(bad)?;
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    if authority.contains('@') {
        return Err(bad());
    }
    let host = match authority.rsplit_once(':') {
        Some((h, port)) if !port.is_empty() && port.chars().all(|c| c.is_ascii_digit()) => h,
        _ => authority,
    };
    if host.is_empty() {
        return Err(bad());
    }
    Ok((scheme.to_ascii_lowercase(), host.to_ascii_lowercase()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allows_only_https_hugging_face_hosts() {
        for ok in ["https://huggingface.co/x/y", "https://us.aws.cdn.hf.co/x", "https://cdn-lfs.hf.co/x?a=b", "https://HuggingFace.co:443/x"] {
            assert_eq!(HuggingFace.allows(ok), Ok(()), "{ok}");
        }
        assert_eq!(HuggingFace.allows("http://huggingface.co/x"), Err(PolicyError::NotHttps));
        for (bad, host) in [
            ("https://example.com/x", "example.com"),
            ("https://huggingface.co.evil.com/x", "huggingface.co.evil.com"),
            ("https://evilhf.co/x", "evilhf.co"),
        ] {
            assert_eq!(HuggingFace.allows(bad), Err(PolicyError::HostNotAllowed(host.into())), "{bad}");
        }
        assert!(matches!(HuggingFace.allows("https://user@huggingface.co/x"), Err(PolicyError::BadUrl(_))));
        assert!(matches!(HuggingFace.allows("huggingface.co/x"), Err(PolicyError::BadUrl(_))));
    }
}
