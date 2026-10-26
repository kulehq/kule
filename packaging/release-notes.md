> **Signed ad hoc.** Kule's macOS app is signed ad hoc and not notarized by
> Apple, so macOS refuses to open it the first time. After you have checked
> the download (below), either open **System Settings → Privacy & Security**
> and choose **Open Anyway** for Kule, or run once:
>
> ```sh
> xattr -dr com.apple.quarantine /Applications/Kule.app
> ```
>
> The same goes for the `kule` command from
> `kule-@VERSION@-macos-universal.tar.gz` downloaded in a browser:
> `xattr -d com.apple.quarantine kule`.
>
> **Windows: not signed.** `Kule_@VERSION@_x64-setup.exe` and
> `Kule_@VERSION@_arm64-setup.exe` carry no code signature, so SmartScreen
> says the publisher is unknown: after you have checked the download
> (below), choose **More info → Run anyway**. The installer installs for
> you alone and asks for no administrator. Windows 11 24H2 with update
> 26100.9278 or later is required; `kule-@VERSION@-windows-<arch>.zip`
> holds the `kule` command with the two programs it starts, to be kept
> together.
>
> **Check the download first.** Every file comes with `SHA256SUMS` and a build
> provenance attestation, which says it was built from this repository by
> this release's workflow:
>
> ```sh
> shasum -a 256 -c SHA256SUMS --ignore-missing
> gh attestation verify Kule_@VERSION@_universal.dmg --repo kulehq/kule \
>   --signer-workflow kulehq/kule/.github/workflows/release.yml \
>   --source-ref refs/tags/v@VERSION@
> ```
>
> Or build it yourself: `cargo build --release -p kule -p kule-desktop`.

