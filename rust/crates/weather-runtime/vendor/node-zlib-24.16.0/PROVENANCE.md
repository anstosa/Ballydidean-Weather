# Node gzip trial source

Unmodified files from the compressor subset of Node.js v24.16.0's
`deps/zlib` at commit `c7d10158bc31036de6783d66beaaaf551e3167aa`
(tag object `75143a8d75629c5d429dd0becb0d725e955f48fb`). Installed Node reports
zlib `1.3.1-e00f703`. The upstream zlib/chromium notices are retained.

This candidate compiles the scalar, prefixed compressor subset only
(`CPU_NO_SIMD`, `HAVE_HIDDEN`, `Z_PREFIX`); no runtime Node, decompressor,
system zlib or new Cargo package is selected. Source integrity is captured
in `source.sha256`. Candidate activation requires exact native x64 and
ARM64 predecessor gzip parity, including retained weather payloads.
