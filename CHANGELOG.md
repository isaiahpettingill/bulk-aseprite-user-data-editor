# Changelog

## 0.1.2

- Use the supplied icon consistently in Linux launcher and window identities.
- Include hicolor icons, desktop metadata and .DirIcon in the AppImage.
- Add a per-user launcher installer for an already downloaded AppImage, with
  an explicit icon path and matching Wayland app ID / X11 WM_CLASS.

## 0.1.1

- Put multi-file Aseprite import first, filter unrelated files before reading bytes,
  preserve existing edits on cancellation, and safely number same-name imports.
- Save visible edits with Ctrl+S, without mandatory preview or apply steps. Native
  saves update originals with backups; browser saves download a ZIP.
- Separate Find/Replace from editing; Ctrl+F and Ctrl+H focus Find, with live match
  counts and scoped user-data or tag-name replacement.
- Replace raw-property JSON as the default interface with typed property rows,
  expandable nested values, and preservation of untouched bulk-edit properties.
- Move the user-data RGBA marker into Advanced; remove selection chevrons while
  retaining highlights and keyboard focus.
- Preserve text when rapid typing and a focus-changing click arrive in one frame.
- Add document redo, safe draft context changes, append-idempotency regressions,
  and browser tests that inspect the actual exported metadata.

## 0.1.0

Initial native and browser metadata editor, safe Aseprite chunk preservation,
recursive import, bulk metadata operations, Windows installer and Linux AppImage.
