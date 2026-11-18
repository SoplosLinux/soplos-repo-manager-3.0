# Changelog

All notable changes to Soplos Repo Manager are documented in this file.

## [3.0.0] - 2026-11-18

First stable release.

### Added

- AppStream metainfo in the eight languages, with the ten screenshots, so the program shows in software centers.
- Manual page (`man soplos-repo-manager`), installed compressed.
- README with the screenshots, the pages, how a repository is published, the security of the tokens and the shortcuts.

## [3.0.0-alpha] - 2026-11-18

### Added

- Skeleton of the 3.0 version, a rewrite with Tauri 2, Rust and Svelte that joins two programs of the 2.x family, both in Python 3 and GTK3: Soplos Repo Manager and Soplos Packager.
- Window with no frame of the system: a card with rounded corners and its own minimize, maximize and close buttons. Only one window of the program opens; a second launch brings the first one to the front.
- Sidebar with the pages in five groups: Repository (Repositories, Packages), Build (Build, Check), Team (Proposals, Send), Publish (Destinations, Accounts) and Tools (GPG keys, Cleanup).
- A banner over every page, with the picture of the program, the icon of the page and its title.
- Light and dark theme that follows the one of the desktop, with a button in the sidebar to switch it.
- Interface in eight languages: English, Spanish, German, French, Italian, Portuguese, Romanian and Russian.
- The text of the interface cannot be selected or copied, and the context menu is off, as in a desktop application. Only the fields where text is typed keep the selection.
- About dialog (F1), Ctrl+Q to quit, Ctrl+1 to Ctrl+9 for the pages, Ctrl+Tab and Ctrl+Shift+Tab to move between pages.
- The icon and the banner picture of the program, drawn in the style of the Soplos 3.0 applications.
- Live job execution bar (`JobBar`) streaming stdout and stderr from git, reprepro, and signing tasks, with detail log drawer and immediate process cancellation (`cancel_current_job`).
- Deep and normal repository cleanup tool (`Cleanup`), allowing package emptying and git history reset to a single clean orphan commit.
- Secure GitHub personal access token (PAT) storage in `~/.config/org.soplos.repomanager/credentials.json` (mode 0600), with automatic authentication injection during remote git push operations to avoid HTTP 403 errors.
- Modern DEB822 repository configuration generator (`.sources` format) and automated bash installation script with public key download and dearmoring to `/usr/share/keyrings/`.
- Sticky action toolbars in Packages, Keys, and Repositories views to keep search and actions accessible during scrolling.
- Team package submission view (`Send`), allowing developers to bundle compiled Debian packages (by file, folder scan or drag & drop), create an isolated proposal branch, push to GitHub, and open an automated Pull Request.
- Team proposal review view (`Proposals`), allowing repository maintainers to view open Pull Requests, inspect included `.deb` packages, accept and publish with GPG signing and reprepro integration, or reject with feedback comments. Includes optional automatic pre-cleaning of existing packages to avoid accumulating obsolete kernel builds.
- Complete localization across all 8 supported languages (English, Spanish, German, French, Italian, Portuguese, Romanian, Russian) for all tools and dialogs.

- Check page: drop .deb packages or folders (gone through with every folder inside) and each one gets a verdict, correct, warnings or errors, with the reasons. It looks at what reprepro would skip (no Section or Priority), at what GitHub refuses (files over 100 MiB, a warning from 50 MiB), at the control fields and the version, and at what software centers need: the menu entry, the icon, and a metainfo whose launchable names a desktop file of the package, with screenshots and a release that matches the version, validated with appstreamcli. lintian runs only when it is asked for and does not decide the verdict. Progress shows in the job bar and it can be cancelled. The packages without errors go to Packages, where its own review decides what is added.
- Build page: a Debian package from one program (installed in /usr/bin), from a program in a folder (installed in /opt with a launcher in /usr/bin) or from a folder laid out as the system. The control file is written from the form (name and version checked as Debian requires, section, priority, dependencies, a description wrapped as Debian writes it, and no field can add a line of its own). The optional desktop integration writes the menu entry, the icon in every size of the hicolor theme and the AppStream metainfo, all named by the same application id. The package is built with root as owner and checked; it can then be added to the repository in use. The maintainer is remembered for the next package.
- Packages marks a .deb with no Section or Priority as not valid in its review, instead of letting reprepro skip it in silence. The package depends on reprepro and curl.

### Changed

- GitHub tokens live only in the keyring of the system (Secret Service: GNOME Keyring, KWallet, KeePassXC). `credentials.json` keeps just the list of accounts. The tokens a previous build wrote there are moved into the keyring the first time the program starts, and they leave the file only after the keyring gives them back. With no keyring, Accounts explains how to install one and does not let an account be added.
- A token never goes on a command line or into a URL: git gets it as an HTTP header through its environment and curl reads it from its standard input. It no longer shows in `ps`, in the log of a job or in the `.git/config` of the repositories, and the remote URLs with a token written by earlier builds are cleaned on the next publication.
- Accounts is the only place for tokens: the token card of Repositories and the quick token of Send are gone, and both point to Accounts.
- The steps a job writes to its log, its title and the texts the program leaves on GitHub (the end of a pull request and the comments when one is accepted or rejected) follow the language of the interface. The error messages of the backend that were in Spanish are in English, like the others.

### Fixed

- Sending a proposal looked for the token in a field that was always empty after the accounts were introduced, so it never found it.
- Replaced full-tree squash push on large repositories with incremental git commits and pushes.
- Fixed job listener initialization lifecycle in the root application component to prevent missing early background job events.
- Added active process PID tracking to immediately terminate running git operations on user cancellation.
- Fixed two-way binding on dynamic type inputs for password/text token visibility toggle.
