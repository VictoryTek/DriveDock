You are helping build a Rust desktop application called **DriveDock**.
DriveDock is a Flatpak-delivered utility for Linux that manages local
and network drives with a modern, simple UI.

==============================
CRITICAL RULE — CONTEXT7
==============================
ALWAYS use **context7** when adding ANY new feature, idea, improvement,
design decision, architectural change, or new capability.
Do not generate new functionality without considering and aligning with context7.

Repeat: Every time you expand this project in any way,
**always use context7**.

==============================
PROJECT GOALS AND REQUIREMENTS
==============================

Primary Purpose:
- Display currently mounted drives
- Allow safe unmounting of drives not actively in use by the OS
- Detect available network drives (SMB, NFS, etc.)
- Allow user to mount network drives
- Provide an option to permanently mount by writing to /etc/fstab
- App must be built and shipped as a Flatpak
- Written in Rust

==============================
UI + UX REQUIREMENTS
==============================
- Modern, minimal, GNOME-friendly UI
- Prefer GTK4 + libadwaita UI in Rust
- Layout idea:
  SECTION 1: "Local Drives"
    - List mounted drives
    - Show device name (sda, sdb1, etc.)
    - Show mount point (/mnt/drive, /run/media/user/disk, etc.)
    - Show filesystem type if available (ext4, xfs, btrfs, etc.)
    - Show size + used if available
    - Provide [Unmount] button where safe

  SECTION 2: "Network Drives"
    - Show detected SMB + NFS shares
    - Show protocol label (SMB / NFS / other)
    - Allow selecting share and clicking [Mount]
    - Provide toggle or checkbox:
      [ ] Persist Mount (write to /etc/fstab)

  SECTION 3: Status + Logs
    - Show helpful errors
    - Show successful mount/unmount feedback
    - Be user-friendly and understandable

==============================
CORE FUNCTIONAL BEHAVIOR
==============================

LOCAL DRIVE HANDLING
- Detect mounted drives using reliable Linux sources
- Ensure no unmount of critical system drives
- Prevent unmount of busy drives
- Graceful error handling

NETWORK DRIVE HANDLING
- Detect SMB shares
- Detect NFS exports
- Allow mounting them
- Auto-create mount points when needed
- Permanent mount option safely appends to /etc/fstab
- Validate before editing fstab
- Prevent duplicate entries
- Align decisions with **context7**

==============================
ENGINEERING EXPECTATIONS
==============================
- Language: Rust
- UI Framework: GTK4 + libadwaita
- Maintainable modular architecture
- Prefer async where appropriate
- Safe Rust first
- Avoid blocking UI
- Reference **context7** when:
  - designing features
  - structuring modules
  - choosing crates
  - planning UX flows

==============================
SECURITY + SAFETY
==============================
- Respect system permissions
- Prefer Polkit for privileged operations
- Warn user before modifying fstab
- Avoid unsafe Rust unless absolutely necessary

==============================
FLATPAK REQUIREMENTS
==============================
- Provide Flatpak manifest
- Properly declare permissions
- Ensure app runs sandboxed
- Allow required filesystem + network access

==============================
DEVELOPER EXPERIENCE
==============================
When generating code:
- Use idiomatic Rust
- Explain architecture when needed
- Suggest appropriate crates
- Avoid meaningless placeholders
- Always ensure new ideas follow **context7**

==============================
INITIAL DELIVERABLES
==============================
1) Rust project bootstrap
2) Basic GTK window
3) Placeholder UI structure
4) Drive listing function
5) Network drive detection scaffold
6) Flatpak build setup skeleton
