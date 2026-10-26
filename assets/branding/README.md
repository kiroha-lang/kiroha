# Kiroha branding assets

The icon uses an ivory K growing into a sage-green leaf on a forest-green background.

| File | Size | Use |
| --- | --- | --- |
| [kiroha-icon.png](kiroha-icon.png) | 1254×1254 | Original icon, avatars |
| [kiroha-icon-128.png](kiroha-icon-128.png) | 128×128 | Website header and small web use |
| [favicon.ico](favicon.ico) | 16×16, 32×32, 48×48 | Multi-size browser favicon |
| [favicon-16.png](favicon-16.png) | 16×16 | Small browser tabs |
| [favicon-32.png](favicon-32.png) | 32×32 | Browser tabs on denser displays |

The favicons preserve the original design and are opaque. MkDocs uses the ICO;
the documentation build publishes the icons and all three favicon files under `assets/`.

For another website, copy the favicon files beside the page and use:

```html
<link rel="icon" href="favicon.ico" sizes="16x16 32x32 48x48">
<link rel="icon" type="image/png" href="favicon-32.png" sizes="32x32">
<link rel="icon" type="image/png" href="favicon-16.png" sizes="16x16">
```

## Regenerate

These are size/format conversions of the original icon, using ImageMagick 6:

```sh
convert assets/branding/kiroha-icon.png -strip -filter Lanczos -resize 128x128 -depth 8 assets/branding/kiroha-icon-128.png
convert assets/branding/kiroha-icon.png -strip -filter Lanczos -resize 16x16 -depth 8 assets/branding/favicon-16.png
convert assets/branding/kiroha-icon.png -strip -filter Lanczos -resize 32x32 -depth 8 assets/branding/favicon-32.png
convert assets/branding/kiroha-icon.png -strip -filter Lanczos -define icon:auto-resize=16,32,48 -depth 8 assets/branding/favicon.ico
```

With ImageMagick 7, use `magick` in place of `convert`. The original image was
made with the built-in image generation tool; its [prompt](kiroha-icon.prompt.txt)
is retained separately.
