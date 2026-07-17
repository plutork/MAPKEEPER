# Cursor alpha (Windows)

Workspace-first alpha for the mapkeeper product shell.

**Audience:** writer / GM with **no prior Git or Cursor** experience.  
**Time:** first run ~30–90 minutes (downloads + MSVC if needed).

---

## 0. What you need

- **Windows 10 or 11** (64-bit).
- **Internet** for downloads and (on a clean repo) `run.ps1` pull.
- **Disk space:** ~2–5 GB for Rust toolchain + build artifacts (after setup).
- A **GitHub account** is optional (public clone works without login).

---

## 1. Install Git (copy the project from GitHub)

Git is the tool that downloads the mapkeeper **source folder** to your PC.

1. Open **[Git for Windows](https://git-scm.com/download/win)**.
2. Download the installer and run it.
3. Accept defaults (Next → Next) unless you know you need something else.
4. **Close and reopen** any terminal or Cursor window after install.

**Check:** open **PowerShell** and run:

```powershell
git --version
```

You should see a version number, not “command not found”.

---

## 2. Install Cursor (the editor)

Cursor is a code editor with a built-in AI assistant — mapkeeper alpha runs **inside** it.

1. Open **[cursor.com/download](https://cursor.com/download)**.
2. Download **Windows** and run the installer.
3. Launch **Cursor**.
4. Sign in when prompted (free tier is enough for alpha).

---

## 3. Get the MAPKEEPER folder on your PC

Repository URL: **<https://github.com/plutork/MAPKEEPER>**

Pick **one** method.

### Method A — Cursor (easiest if you already have Cursor)

1. In Cursor: **File → New Window**.
2. If you see **Clone repo** on the welcome screen, click it.  
   Otherwise: **Ctrl+Shift+P** → type **Git: Clone** → Enter.
3. Paste: `https://github.com/plutork/MAPKEEPER.git`
4. Choose a parent folder (e.g. `Documents` or `C:\projects`).
5. When asked, **Open** the cloned folder.

You should see files like `README.md`, `setup.ps1`, `run.ps1` in the left sidebar.

### Method B — PowerShell (classic git clone)

1. Open **PowerShell** (not CMD).
2. Go where you keep projects, then clone:

```powershell
cd $HOME\Documents
git clone https://github.com/plutork/MAPKEEPER.git
cd MAPKEEPER
```

3. In Cursor: **File → Open Folder…** → select the `MAPKEEPER` folder you just cloned.

### Method C — GitHub Desktop (GUI, no terminal for clone)

1. Install **[GitHub Desktop](https://desktop.github.com/)**.
2. **File → Clone repository → URL**.
3. URL: `https://github.com/plutork/MAPKEEPER.git`
4. Choose local path → **Clone**.
5. **Repository → Open in Cursor** (or open that folder in Cursor via **File → Open Folder**).

---

## 4. First-time workspace setup

You must open the **MAPKEEPER repo root** (the folder that contains `Cargo.toml` and `run.ps1`).

1. In Cursor, open the terminal: **Terminal → New Terminal** (or **Ctrl+`**).
2. Confirm the prompt path ends with `\MAPKEEPER` (or your clone name).
3. Run:

```powershell
.\setup.ps1
```

- The script **asks before** heavy installs (Rust via winget, etc.).
- If the desktop build reports an MSVC error, install Build Tools with the C++ workload manually.
- If setup tells you to restart the terminal, close the terminal panel and run `.\setup.ps1` again.

You only need **`setup.ps1` once** per machine (or after wiping the toolchain).

---

## 5. Build and launch (daily)

From the same repo root in Cursor’s terminal:

```powershell
.\run.ps1
```

- On a **clean** git tree, `run.ps1` may **pull** updates, then rebuilds the web UI and opens the **desktop app**.
- If the tree is **dirty** (you edited files), pull is skipped — that is normal.

When the app opens:

- On **empty Home**, click **Create your first world**.
- Worlds are stored under **`Documents/MAPKEEPER Worlds`** by default (not inside this repo).
- Creation writes identity + immutable `[spatial]` config and
  `spatial/state.json`; pick a **map size** card (Default ≈2k cells; size fixed
  after create). Opens the five-mode product shell.
  In **Editor**, tools sit on a strip under the mode tabs:
  **View** (default) and **Relief**. Left panel holds the active tool.
  **View** — drag to pan, wheel to zoom; layers **Empty** (contour grid) or
  **relief** (elevation tint). **Relief** — Raise/Lower with an adjustable
  hard-disk brush (**Stamp** default, opt-in **Airbrush** + Rate) and
  **Edit ocean** on the left. Height **0** is the ocean datum; with Edit ocean
  off, ocean cells (`h < 0`) stay frozen and land Lower floors at 0; turn Edit
  ocean on to dig or fill the sea. Cell heights clamp **−60…100**.
  The right panel is reserved for later details. View settings are not saved.
  Reload re-reads disk; screen/camera are never persisted. This is the active
  thin map binding; domain catalogs, generators, and History still need Shape.

---

## 6. Daily cheat sheet

| When | Command |
|------|---------|
| Normal day | `.\run.ps1` |
| Update without launching | `.\update.ps1` (stops if git tree is dirty) |
| Something broke | In Cursor chat, run **`/doctor`** (agent helps with consent) |

No installer download. No agent required for the happy path.

---

## 7. What this validates

Open the MAPKEEPER workspace → prepare/run the desktop product shell from
source → create/resume a portable world folder (identity + spatial state) →
paint relief in Editor → switch among Editor, Generator, Wizard, Agent, and
History without loading map-v2.

---

## Safety

- `setup.ps1` asks before heavy changes; never silent-installs MSVC; does not git pull.
- `run.ps1`: on a **clean** tree, fetch + `git pull --ff-only` when behind upstream; **dirty** tree skips pull; pull/fetch failure **stops** before build; always rebuilds web + launches; no toolchain installs.
- `update.ps1` stops on a dirty tree; pull + rebuild only (no launch).
- `/doctor` asks before heavy installs; never deletes world folders.

---

## Not this alpha

- NSIS / SmartScreen installer-first distribution
- Portable runtime zip
- In-app Check for updates
- `/mk-*` commands or `doctor.ps1`
- macOS / Linux root scripts

Contributor details: [`DEV.md`](DEV.md).
