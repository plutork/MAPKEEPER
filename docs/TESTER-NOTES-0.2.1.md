# mapkeeper v0.2.1 (alpha) — tester notes

Thank you for testing the installable mapkeeper desktop alpha.

## What this build is

- Windows desktop alpha of mapkeeper (writer/GM world editor)
- Local-first: worlds stay on your machine
- Early build: focused on install, launch, create/open flow
- Includes Home `Check for updates` button (D-76)

## SmartScreen

This installer is unsigned for alpha. Windows may show SmartScreen:

1. Click **More info**
2. Click **Run anyway**

## Smoke checklist

1. Download installer from release assets (`mapkeeper_0.2.1_x64-setup.exe`)
2. Install for current user
3. Launch from Start Menu or desktop shortcut
4. Confirm no cargo/git/browser/localhost is required
5. On Home, confirm `MAPKEEPER 0.2.1` and click **Check for updates**
6. Create a blank world from Home and open editor once
7. Quit and relaunch app; confirm world is still listed on Home
8. Uninstall app

Expected after uninstall: app is removed; your created world folder may remain on disk.

## Feedback

Please report:

- install/launch failures
- SmartScreen or antivirus friction
- update-check button behavior
- create/open/relaunch problems
- uninstall surprises
