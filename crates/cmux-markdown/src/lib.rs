//! `cmux-markdown` — the headless, mount-independent core of the markdown viewer,
//! ported faithfully from the canonical macOS Swift sources under
//! `Sources/Panels/Markdown*.swift`.
//!
//! Eight pure pieces live here today:
//!
//! - [`file_link`] — [`file_link::resolve`] / [`file_link::is_markdown_path_like`],
//!   a verbatim port of `MarkdownPanelFileLinkResolver` (`MarkdownPanelFileLinkResolver.swift`):
//!   the markdown-extension gate, candidate-path resolution, lexical
//!   standardization, and the on-disk existence check that decides whether an
//!   in-document link points at a local markdown file.
//! - [`local_image_jail`] — [`resolve_local_image`], a port of the
//!   `cmux-local-image://` path jail (`MarkdownWebRenderer.swift`): the
//!   security boundary that admits an on-disk image only when it lives strictly
//!   under the markdown document's directory and has an allowed image extension.
//! - [`theme`] — [`MarkdownWebTheme`], a port of `MarkdownWebTheme` +
//!   the `NSColor` markdown color math (`MarkdownWebSupport.swift`): the six
//!   GitHub-CSS theme variables derived from the background color via WCAG
//!   contrast targeting.
//! - [`assets`] — [`assets::MarkdownViewerAssets`], a port of
//!   `MarkdownViewerAssets` (`MarkdownViewerAssets.swift`): the six-placeholder
//!   `shell.html` template substitution ([`assets::MarkdownViewerAssets::shell_html`]),
//!   the lazy asset cache, and the remote-image `localizedStringsJSON` map.
//! - [`remote_image`] — [`remote_image_url`] + the SSRF gate for the
//!   `cmux-remote-image://` scheme (`MarkdownRemoteImageLoader.swift`): HTTPS-only,
//!   no-userinfo, default-port, hostname/IP-literal allowlist rejecting
//!   loopback/private/link-local/CGNAT/reserved ranges, MIME canonicalization,
//!   and HTTP request framing. DNS resolution + the TLS fetch stay in the host
//!   layer, which screens each resolved address via `is_allowed_resolved_ip`.
//! - [`remote_image_loader`] — the pure byte-level HTTP response processing for
//!   remote images (`MarkdownRemoteImageLoader.swift`): the chunked-body decoder
//!   ([`remote_image_loader::decode_chunked_body`]), the header parser /
//!   status classifier ([`remote_image_loader::parse_headers`]), the host-fed
//!   streaming accumulator ([`remote_image_loader::RemoteImageAccumulator`]), and
//!   the redirect-follow decision ([`remote_image_loader::redirect_decision`]).
//!   The socket read loop, TLS, timeouts, and DNS pinning stay in the host layer.
//! - [`typography`] — [`MarkdownTypography`] + the `font_size` / `max_width` /
//!   `font_family` domain logic (`MarkdownFontSizeSettings.swift` et al.): clamp
//!   ranges, `page_zoom`, CSS `font-family` escaping, and the defaults
//!   orchestration, kept in sync with `cmux_config::MarkdownConfig`.
//! - [`mention_link`] — [`mention_link::link`], a port of `TextBoxMentionMarkdown`
//!   (`TextBoxMentionMarkdown.swift`): the composer mention-link builder (label
//!   escaping + path angle-wrapping/percent-encoding).
//!
//! Everything requiring a live `WKWebView` / GPU / AppKit (the renderer wiring,
//! the remote-image consent UI, typography *controls*) stays in the GUI layer and
//! is out of scope for this crate.

pub mod assets;
pub mod file_link;
pub mod local_image_jail;
pub mod mention_link;
mod path_util;
pub mod remote_image;
pub mod remote_image_loader;
pub mod theme;
pub mod typography;

pub use assets::MarkdownViewerAssets;
pub use local_image_jail::{resolve_local_image, ResolvedLocalImage, LOCAL_IMAGE_URL_SCHEME};
pub use remote_image::{remote_image_url, REMOTE_IMAGE_URL_SCHEME};
pub use remote_image_loader::{
    decode_chunked_body, parse_headers, redirect_decision, HeaderOutcome,
    MarkdownRemoteImageFetchResult, Outcome, ProcessResult, RemoteImageAccumulator,
};
pub use theme::MarkdownWebTheme;
pub use typography::MarkdownTypography;
