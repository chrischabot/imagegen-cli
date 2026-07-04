---
name: imagegen
description: >-
  Generate or edit images with OpenAI's gpt-image-2 via the `imagegen` CLI.
  Use when the user asks to create, generate, draw, render, or edit an image,
  illustration, icon, logo, banner, hero image, placeholder art, texture,
  diagram illustration, or photo — or to restyle/modify an existing image file.
---

# Generating images with imagegen

`imagegen` generates and edits images with OpenAI's gpt-image models
(default: `gpt-image-2`). It prints saved file paths to stdout (one per line);
progress goes to stderr. Requires `OPENAI_API_KEY` (or `--api-key`).

## Generate

```bash
imagegen generate "a watercolor robot holding a paintbrush" -o robot.png
```

Key flags (all optional):

| Flag | Values | Notes |
|---|---|---|
| `-o, --out` | file or dir | default: auto-named file in cwd; with `-n 2+` a file path becomes `name-1.png`, `name-2.png` |
| `-s, --size` | `auto`, `WIDTHxHEIGHT` | gpt-image-2: edges multiples of 16, max edge 3840, ratio ≤ 3:1, e.g. `1024x1024`, `1536x1024`, `2048x1152`, `3840x2160` |
| `-q, --quality` | `auto`, `low`, `medium`, `high` | `low` ≈ $0.006 and ~15s; `high` ≈ $0.21 and slower. Use `low` for drafts/placeholders, `medium`+ for final assets |
| `-f, --format` | `png`, `jpeg`, `webp` | jpeg/webp are smaller and faster; add `-c 0-100` for compression |
| `-n` | 1–10 | variations of the same prompt in one call |
| `-m, --model` | `gpt-image-2` (default), `gpt-image-1.5`, `gpt-image-1`, `gpt-image-1-mini` | use `gpt-image-1.5` if you need `--background transparent` (gpt-image-2 doesn't support it) |
| `--json` | | machine-readable result: absolute paths, bytes, token usage |
| `--quiet` | | paths only, no progress on stderr |

## Edit / restyle / combine existing images

```bash
imagegen edit "make the background a sunset beach" -i photo.png -o edited.png
imagegen edit "combine into one scene" -i cat.png -i hat.png -o combined.png
imagegen edit "replace masked area with a garden" -i room.png --mask mask.png -o out.png
```

Masks are PNG with alpha; transparent pixels mark the region to replace.

## Recipes

- Draft cheaply, then upscale the winner:
  `imagegen gen "..." -q low -n 3 -o drafts/` → pick one → regenerate at `-q high`
- Web asset: `imagegen gen "..." -f webp -c 80 -s 1536x1024`
- Wallpaper/hero: `-s 3840x2160 -q high` (slow; can take 1–2 min)
- Icon on transparent background: `-m gpt-image-1.5 -b transparent -f png`
- Verify results by reading the output file (it's an image; you can view it).

## Exit codes

`0` success · `1` API/network error · `2` no/invalid API key ·
`3` blocked by moderation (rephrase the prompt; do not retry verbatim) ·
`4` bad arguments or missing input file.

## Notes

- Costs real money per image (token-based; `--json` reports usage). Prefer
  `-q low` unless the user wants final quality.
- Prompts: be specific about style, medium, composition, lighting, and
  background. The model follows detailed prompts well.
- `imagegen models` lists image models available to the current key.
