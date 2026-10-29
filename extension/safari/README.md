# Safari extension source

`Shared/` contains the Safari-compatible extension manifest and background script. This repository does not contain an `ExFetch.xcodeproj` or a signed Safari extension application.

To evaluate the source, use Xcode's Safari Web Extension converter to create a local app project from `Shared/`, build and sign that generated project with your own Apple development configuration, then enable it in Safari's Extensions settings. See [Apple's Safari Web Extension documentation](https://developer.apple.com/documentation/safariservices/safari_web_extensions).

The browser-policy regression tests exercise this shared JavaScript source. They do not validate Xcode conversion, signing, packaging, or Safari runtime behavior. The Chrome unpacked extension is the documented starting point in the [root README](../../README.md).
