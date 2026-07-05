# Inbox — Zed One Dark · Violet Terminal Mono

Self-contained HTML/CSS export. No build step, no dependencies.

## Contents
- `index.html` — full 3-column inbox markup
- `styles.css` — design tokens (Zed One Dark) + all component styles
- `fonts/` — self-hosted webfonts (woff2, latin subset)
  - JetBrains Mono 400 & 500 (UI, mono chrome)
  - Inter variable (mail body, subject headline)

## Usage
Open `index.html` directly in any modern browser, or drop the folder
onto a static host (Netlify, Vercel, GitHub Pages, S3, …). All assets
are relative.

## Design tokens
```
--background   #282c34    Zed editor background
--surface      #21252b    panels
--accent       #c678dd    One Dark magenta / violet
--text         #dcdfe4    body text
--text-strong  #ffffff    sender · subject · mail body
--muted        #8a929e    meta, timestamps, snippets
```

Icons are inline Phosphor Thin SVGs (MIT).
