# Desktop installation and updates

[Documentation](README.md) · [Developer README](../README.md)

## Install a packaged release

Download the installer or AppImage from [Releases](https://github.com/Ketchup3D/app/releases).
Workflow artifacts are also available on the run's summary page for 14 days.
On Linux, keep the AppImage in a user-writable location, run `chmod +x` on it,
and launch it. AppImage mounting may require your distro's FUSE 2 compatibility
package; the GPU/display runtime requirements in the [developer README](../README.md#prerequisites) still apply. Do not extract
the AppImage or run a bare executable if you want automatic updates.

## Automatic desktop updates

The packaged app checks public GitHub Releases in the background at most once
every 24 hours, including during long editor sessions. It downloads newer
versions for its own OS/architecture and verifies package size and checksums.
Full packages are published initially; delta packages are not required. No
update server, end-user GitHub login, or embedded GitHub token is needed.

**Help → Check for updates…** bypasses the daily limit and opens the update
window. That window also controls automatic checking/downloading. Disabling it
stops future automatic checks; a check already in progress may finish, and an
already downloaded update will still install on the next launch. Failed checks
leave the app usable and can be retried manually. The **Update ready** button
appears after a download completes. Installation happens on the next launch,
or through **Restart now**. KetchUp3D does not force a restart during editing.

Restart now offers to save modified work as a project file (`.k3d` by default;
`.ketchup` is also supported), or keep it in local recovery. It saves the model
and its project-file association before
closing; a save failure prevents restart. Finish drawing/modeling previews
before restarting. Autosave, update preferences, and restart information stay
in the existing application data directory outside the installed application.
`KETCHUP_DATA_DIR` redirects them for testing. Browser builds do not include
Velopack; development builds and unpackaged executables do not contact the
update service.

Users of older bare executables must manually install one updater-enabled
release. Subsequent releases update through Velopack. GitHub's unauthenticated
API limits apply, so networks with many users behind one IP may need retries;
the update source can later move to a static HTTPS feed.

## Release notes

Open **Help → Release notes** to browse the release history. The modal works offline, opens on the latest release, and lets you
browse every release using the version picker or **Newer** / **Older** buttons.
On the first launch of a new version, the release notes open automatically if
you haven't viewed them yet. The last viewed release version is saved immediately
across app restarts and browser reloads, so the modal appears automatically only
once per release. You can reopen it at any time from **Help → Release notes**.
