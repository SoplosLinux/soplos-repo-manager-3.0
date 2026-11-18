# Changelog

All notable changes to Soplos Repo Manager are documented in this file.

## [3.0.0] - 2026-11-18

First stable release.

### Added

- AppStream metainfo in the eight languages, with the ten screenshots, so the program shows in software centers.
- Manual page (`man soplos-repo-manager`), installed compressed.
- README with the screenshots, the pages, how a repository is published, the security of the tokens and the shortcuts.

### Changed

- The toolbar of Send looks at the room it has, not at the width of the window: the destination and the buttons share one row while their measured widths fit, and below that they take two cells that fill the bar, so the buttons grow instead of leaving empty space. The destination is short on purpose and cuts a long name.
- Sending a proposal is made with the GitHub API and no longer with Git: each `.deb` goes up as a blob, a tree is made over the one of the base branch, the commit is created there and the branch is a reference to it. Only the package travels; nothing of the repository is downloaded, so a destination that holds gigabytes of packages costs the package itself.
- Accepting a proposal no longer fetches its branch: the packages of the pull request are downloaded with the GitHub API, by the commit of the proposal, so it also works when the proposal comes from a fork. The branch is removed through the API instead of a `git push --delete` in the local copy.
- The green notices of Proposals, Send and Cleanup go away by themselves after five seconds and have a close button, instead of staying on the page.

### Fixed

- A proposal could not be sent to a large repository: the base branch was downloaded whole before anything else — with git, even a clone without files made Git ask for every missing object when writing the tree — so the job never reached the pull request and no error explained why.
- The pull request is created with the owner in its head (`owner:branch`), so a collaborator account can open it; when the branch already has one, its number is returned instead of failing.
- A refused pull request now shows the reason GitHub gives, with the detail of a validation failure, instead of a generic message.
- Every request to the GitHub API looks at the answer: when GitHub refuses it (no permission, not found, validation), the job fails with the reason GitHub gives. Before, a refusal was taken as done: rejecting a proposal with a token that could not close pull requests said "rejected" while the pull request stayed open.
- Accepting a proposal never publishes an empty or half repository. Every package is checked before the repository is touched (a valid package, with Section and Priority, for an architecture of the repository); with "clean first" the packages the repository had are set aside next to it and put back if anything fails before publishing, and a package reprepro refuses stops the job instead of being a warning.
- Accepting a proposal makes the data of the software centers (DEP-11) and signs the Release again, as adding packages does, so an application in a proposal shows in the Store.
- Accepting a proposal reads every file of the pull request, page after page, and not only the first hundred; the list of proposals shows all of them too.
- When accepting fails half way, the job bar says so and the temporary folder of the packages is removed, instead of staying "running" and leaving the folder in the repository.
- Rejecting a proposal also deletes its branch, as accepting does.
- Texts that showed their key or a fixed Spanish sentence: the save button of a destination, the loading line of Destinations and Accounts, the copy button of a destination and the description placeholder, now in the eight languages.

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
