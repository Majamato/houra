# Releasing Houra

Houra ships through five channels, all built from the same two release tarballs
(the source and the vendored crates) that `scripts/make-release-tarballs.sh` creates:

| Channel | For | Built by | Published by |
|---|---|---|---|
| GitHub Release assets: binary tarball, `install.sh`, `.deb`, `SHA256SUMS` | Any distribution; Debian | `.github/workflows/release.yml` | You, by publishing the draft |
| Fedora COPR | Fedora 44 | COPR, from `.copr/Makefile` | You, by starting the COPR build |
| Ubuntu PPA `ppa:majamato/houra` | Ubuntu 26.04 and 26.10 | Launchpad, from source packages CI prepares | You, by signing and uploading them |
| AUR `houra-bin` | Arch and derivatives | `.github/workflows/published.yml` | The workflow, with the AUR key secret |

Pushing a `vX.Y.Z` tag runs the release workflow. It builds every package, installs
each one on the distributions it targets (see CONTRIBUTING's
[Packages](../CONTRIBUTING.md#packages)), and drafts the GitHub Release. Nothing is
public until you publish the draft.

## One-time setup

### GitHub

1. Make the repository public (Settings → General → Danger Zone → Change visibility).
2. On the repository page, click the gear next to **About** and set:
   - Description: `Know where your hours go: a time tracker for GNOME`
   - Website: leave empty, or link the COPR project once it exists
   - Topics: `gnome`, `gtk4`, `libadwaita`, `rust`, `time-tracker`
3. Settings → General → Social preview: upload `data/screenshots/tracker.png`.
4. Settings → Actions → General: allow GitHub Actions, and under **Workflow
   permissions** keep "Read repository contents". The release workflow asks for write
   access itself.

### COPR

1. Create a Fedora account at <https://accounts.fedoraproject.org/> and log in to
   <https://copr.fedorainfracloud.org/> with it.
2. Create a new project named `houra`:
   - Chroots: tick **fedora-44-x86_64** only. Houra is tested only there, so this
     is the only place people can install it from.
   - Description: `Know where your hours go: a time tracker for GNOME with a top-bar timer.`
   - Instructions: `sudo dnf copr enable majamato/houra && sudo dnf install houra`
   - Leave "Enable internet access during builds" off. Only the source RPM step
     downloads crates, and COPR always gives that step network access; the RPM itself
     builds offline from the vendored crates.
3. In the project, open **Packages → New package → SCM** and fill in:
   - Package name: `houra`
   - Clone URL: `https://github.com/Majamato/houra.git`
   - Committish: leave empty (each build names the tag)
   - Subdirectory: leave empty
   - Spec file: `packaging/fedora/houra.spec`
   - Type: `git`
   - SRPM build method: **make_srpm**
4. Optional: COPR can build automatically when you push. In the package settings tick
   **Webhook rebuild**, then copy the GitHub webhook URL from the project's **Settings →
   Integrations** and add it in GitHub under Settings → Webhooks (content type
   `application/json`). COPR then builds on every push, including pushes to `main`
   that are not releases, so starting builds by hand (step 5 below) is simpler at first.

### Launchpad (Ubuntu PPA)

Uploads to a PPA must be signed with a GPG key registered on Launchpad. The key stays
on your machine; CI never sees it.

1. Create an account at <https://launchpad.net/> and sign the Ubuntu Code of Conduct
   if Launchpad asks for it.
2. Create a signing key whose email is the one in `packaging/debian/changelog`
   (`mariojmt9@gmail.com`):

   ```sh
   gpg --full-generate-key          # RSA and RSA, 4096 bits
   gpg --list-secret-keys --keyid-format=long
   gpg --keyserver keyserver.ubuntu.com --send-keys <KEY-ID>
   ```

3. On your Launchpad profile, open **OpenPGP keys**, paste the fingerprint
   (`gpg --fingerprint <KEY-ID>`), and decrypt the email Launchpad sends to confirm it.
4. On your profile, choose **Create a new PPA**: URL `houra`, display name `Houra`,
   description `Know where your hours go: a time tracker for GNOME with a top-bar timer.`
   Then open **Change details** and keep only **AMD x86-64** under processors.
5. Install the signing and upload tools: `sudo dnf install devscripts dput`. If `dput`
   is not packaged for your Fedora, run it from a container:
   `podman run --rm -it -v "$PWD:/w:z" -w /w docker.io/library/ubuntu:26.04 bash -c
   'apt-get update && apt-get install -y dput && dput ppa:majamato/houra *_source.changes'`.

### AUR

1. Create an account at <https://aur.archlinux.org/> and add an SSH public key to it
   (**My Account → SSH Public Key**). Use a key made only for this:
   `ssh-keygen -t ed25519 -f ~/.ssh/aur -C houra-aur`.
2. In GitHub, add the private key as the repository secret `AUR_SSH_PRIVATE_KEY`
   (Settings → Secrets and variables → Actions). Without it, the workflow still builds
   and checks the package and attaches its files as an artifact, but does not publish.
3. The first push creates the AUR package `houra-bin`; nothing else is needed.

## Per release

Replace `X.Y.Z` with the new version.

1. **Bump the version** in all six places. `scripts/make-release-tarballs.sh` refuses
   to run if they disagree.
   - `Cargo.toml`: `version = "X.Y.Z"` under `[workspace.package]`, then run
     `cargo check --workspace` so `Cargo.lock` picks it up.
   - `meson.build`: `version: 'X.Y.Z'`.
   - `packaging/fedora/houra.spec`: `Version: X.Y.Z`, `Release: 1%{?dist}`, and a new
     `%changelog` entry at the top, for example
     `* Wed Sep 30 2026 majamato - X.Y.Z-1`. The weekday must match the date
     (`date '+%a %b %d %Y'` prints it).
   - `data/io.github.majamato.Houra.metainfo.xml.in`: add a `<release version="X.Y.Z"
     date="YYYY-MM-DD">` above the previous one, with a short description of what
     changed, and point the screenshot URLs at `vX.Y.Z`.
   - `packaging/debian/changelog`: a new entry at the top targeting `unstable`, for
     example `houra (X.Y.Z-1) unstable; urgency=medium`, with the same maintainer line
     as before and today's date from `date -R`. `scripts/make-deb.sh` rewrites it per
     Ubuntu series.
   - `packaging/arch/PKGBUILD`: `pkgver=X.Y.Z` and `pkgrel=1`. Leave
     `sha256sums=('SKIP')`; the release workflow fills it in.

   New or changed text in the metainfo needs translating: see
   [Translations](../CONTRIBUTING.md#translations).

2. **Run all checks** from [CONTRIBUTING.md](../CONTRIBUTING.md#checks), including the
   metadata validation. Then check that the release tarballs build:

   ```sh
   git commit -am "Release X.Y.Z"
   scripts/make-release-tarballs.sh /tmp/houra-release HEAD
   ```

3. **Tag and push:**

   ```sh
   git tag -a vX.Y.Z -m "Houra X.Y.Z"
   git push origin main vX.Y.Z
   ```

4. **Check the draft release.** The **Release** workflow (Actions tab) builds and
   tests every package and drafts the release with these assets:
   `houra-X.Y.Z.tar.xz`, `houra-X.Y.Z-vendor.tar.xz`, `houra-X.Y.Z-x86_64-linux.tar.xz`,
   `houra_X.Y.Z-1_amd64.deb`, `install.sh` and `SHA256SUMS`. The spec's `Source0` and
   `Source1` and the PKGBUILD point at these names, so they must not change. Replace the
   generated notes with the metainfo `<release>` items plus the install commands from
   the README, then **Publish release**.

   Publishing runs the **Published release** workflow. It pushes `houra-bin` to the
   AUR and installs the release with the README's `curl … | bash` command on Fedora.

5. **Upload to the PPA.** Download the `ppa` artifact from the Release workflow run
   (`gh run download <run-id> -n ppa -D ppa`), then sign and upload each series:

   ```sh
   cd ppa
   debsign -k <KEY-ID> houra_X.Y.Z-1~ubuntu26.04.1_source.changes houra_X.Y.Z-1~ubuntu26.10.1_source.changes
   dput ppa:majamato/houra houra_X.Y.Z-1~ubuntu26.04.1_source.changes
   dput ppa:majamato/houra houra_X.Y.Z-1~ubuntu26.10.1_source.changes
   ```

   Launchpad emails you when each upload is accepted, and builds take 15–30 minutes;
   follow them on the PPA's **View package details** page. Both series reuse the same
   orig tarballs, which must be the release's: build the source packages only from the
   downloaded release tarballs (`scripts/make-deb.sh RELEASE_DIR OUT --series CODENAME`).

   When a new Ubuntu release comes out, add its codename and version to
   `UBUNTU_SERIES` in `scripts/make-deb.sh` and to the loop in
   `.github/workflows/packages.yml`, and drop series that reached end of life.

6. **Trigger the COPR build** (skip this if the webhook already did): in the COPR
   project open **Packages → houra → Rebuild**, set Committish to `vX.Y.Z`, and
   submit. The build log is under **Builds**; it takes several minutes. If it fails,
   read `builder-live.log` for the failing step.

## Test the package on this machine

A development setup shadows parts of the installed package, so clear it first:

1. Quit Houra (<kbd>Ctrl</kbd>+<kbd>Q</kbd>).
2. Remove the development copy of the extension, which takes precedence over the
   system one:

   ```sh
   rm -rf ~/.local/share/gnome-shell/extensions/houra@majamato.github.io
   ```

3. Remove the development autostart entry, which starts the binary from your build
   directory at login:

   ```sh
   rm -f ~/.config/autostart/io.github.majamato.Houra.desktop
   ```

   The installed Houra creates a new one pointing at `/usr/bin/houra` if "Open Houra
   when signing in" is on.

4. Remove any copy installed with the install script:

   ```sh
   ~/.local/share/houra-installer/install.sh --uninstall
   ```

5. Remove the local release launcher, which takes precedence over the package's
   launcher:

   ```sh
   rm -f "${XDG_DATA_HOME:-$HOME/.local/share}/applications/io.github.majamato.Houra.desktop"
   ```

6. Install from COPR:

   ```sh
   sudo dnf copr enable majamato/houra
   sudo dnf install houra
   ```

7. Start Houra from the Activities overview, then log out and back in.
8. Confirm that the top bar shows Houra, that the extension appears as enabled in the
   Extensions app, and that `rpm -qf /usr/share/gnome-shell/extensions/houra@majamato.github.io`
   names the `houra` package. Start and pause a timer from the top bar.

Your existing data in `~/.local/share/houra/` carries over; the package reads the same
database as the development build.

## Later: Flathub

Flathub would need two separate deliverables: an app-only Flatpak (a Flatpak cannot
install or enable a GNOME Shell extension), and the extension published on
[extensions.gnome.org](https://extensions.gnome.org/). Idle detection, sleep handling
and autostart would also need rework for the sandbox. None of this is needed for COPR.
