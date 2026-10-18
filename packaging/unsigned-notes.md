> **A preview, signed ad hoc.** The macOS app is not notarized by Apple yet,
> so macOS refuses to open it the first time. After you have checked the
> download (below), either open **System Settings → Privacy & Security** and
> choose **Open Anyway** for Kule, or run once:
>
> ```sh
> xattr -dr com.apple.quarantine /Applications/Kule.app
> ```
>
> **Check the download first.** Every file comes with `SHA256SUMS` and a build
> provenance attestation, which says it was built from this repository by
> this release's workflow:
>
> ```sh
> shasum -a 256 -c SHA256SUMS --ignore-missing
> gh attestation verify Kule_@VERSION@_universal.dmg --repo kulehq/kule
> ```
>
> Or build it yourself: `cargo build --release -p kule -p kule-desktop`.

