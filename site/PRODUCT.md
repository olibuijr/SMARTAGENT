# PRODUCT.md — smartagent.olibuijr.com

## Register

Brand. A single-page marketing/landing surface where the design IS the product: a playable pixel-game world, not a scrolling page.

## What this is

The public landing site for SMARTAGENT — a pure-Rust, zero-dependency AI agent platform (27 crates, 22 tools, 8-agent autonomous fleet). The site is a Super-Mario-Bros-like side-scrolling world rendered with three.js WebGPU (WebGL fallback), where the visitor walks past the fleet's eight legendary-developer pixel avatars, bumps ?-blocks for facts, and reaches the GitHub castle at the end. HTML is deliberately minimal: a canvas plus hidden semantic content for accessibility/SEO.

## Target users

Developers and technically curious visitors who clicked a link from GitHub, Telegram, or a talk. Desk context, often after dark, 10–60 seconds of attention unless the page earns more. The job: understand what SMARTAGENT is, smile, and click through to the repo.

## Brand personality

Playful · handcrafted · uncompromisingly technical. The same personality as the TUI: pixel-art avatars, terminal colors, everything built from scratch.

## Aesthetic lane (named, deliberate)

8-bit console night-world. This is NOT a reflex lane choice: the product's existing brand is literal pixel art (the fleet's sextant avatars in the TUI sidebar), so the game world is identity-preservation. Reference: SMB 1-1 pacing crossed with the calm of a terminal at night.

## Palette (from committed brand assets — extensions/agentpanel.ts)

- World/panel bg: `#262626` (the TUI panel background), night-sky bands darker.
- Skin: `#FCCDA5`; blush `#FAAA96`; mouth `#C35F55` (from .scratch/gen-faces.ts).
- Agent accents (TUI ACCENTS xterm → hex): `#ff87d7`, `#5fd7d7`, `#afd787`, `#ffaf5f`, `#af87ff`, `#87d7ff`.
- Strategy: Committed — dark drench with one saturated accent per agent zone.

## Typography

Press Start 2P (vendored woff2), drawn onto canvas textures in-world. The single family is the voice; no pairing needed — this is a game cartridge, not an article.

## Anti-references (what this must NOT be)

- A normal landing page with hero/features/pricing sections.
- SaaS-cream, editorial-serif, glassmorphism, gradient text.
- A tech-demo with no information content — the world must actually explain SMARTAGENT.

## Accessibility

Hidden semantic DOM mirrors all in-world content (team roster, facts, repo link). `prefers-reduced-motion` disables idle bobbing, star twinkle, and auto-demo walking. Keyboard-first controls; touch supported.

## Constraints

- Minimal HTML (canvas + a11y content). All UI drawn in-world.
- three.js WebGPU build, vendored locally (no CDN at runtime); WebGL fallback automatic.
- Static files only — deployed to nginx on EC2 (akurai-mail), no build step.
