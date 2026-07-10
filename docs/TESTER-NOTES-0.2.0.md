# mapkeeper v0.2.0 (alpha) — tester notes

Thank you for testing the first installable mapkeeper desktop alpha.

## What this build is

- Windows desktop alpha of mapkeeper (writer/GM world editor)
- Local-first: worlds stay on your machine
- Early build: focused on install, launch, create/open flow

## SmartScreen

This installer is unsigned for alpha. Windows may show SmartScreen:

1. Click **More info**
2. Click **Run anyway**

## Smoke checklist

1. Download installer from release assets (`mapkeeper_0.2.0_x64-setup.exe`)
2. Install for current user
3. Launch from Start Menu or desktop shortcut
4. Confirm no cargo/git/browser/localhost is required
5. Create a blank world from Home and open editor once
6. Quit and relaunch app; confirm world is still listed on Home
7. Uninstall app

Expected after uninstall: app is removed; your created world folder may remain on disk.

## Feedback

Please report:

- install/launch failures
- SmartScreen or antivirus friction
- create/open/relaunch problems
- uninstall surprises
