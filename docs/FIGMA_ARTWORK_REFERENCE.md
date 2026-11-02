# Compare an editable import against its Figma PNG

The optional `figma-artwork` browser scenario uses frame `1:308` from *Mobile Apps – Prototyping Kit* by Renata Pôrto. Daeheon Jeong, Seoyeon Byun, Kihoon Son, Dae Hyun Kim and Juho Kim published the paired SVG and PNG as CANVAS replication sample `gid1-11` under CC BY 4.0. The paper describes exporting both formats through Figma’s REST API. The dataset keeps the corresponding Figma JSON and source metadata. [Original design](https://www.figma.com/community/file/1129468881607079432) · [Dataset revision](https://huggingface.co/datasets/kixlab/CANVAS/tree/004d47e0938e6f4f30dcec06f6147c15ebdd0c60/replication_gt) · [Research paper](https://arxiv.org/html/2511.20737v1) · [License](https://creativecommons.org/licenses/by/4.0/)

From the repository root, with the browser build and Playwright installed, download the two source files and run the comparison.

```sh
mkdir -p /tmp/ferrodesign-figma-reference
source_url=https://huggingface.co/datasets/kixlab/CANVAS/resolve/004d47e0938e6f4f30dcec06f6147c15ebdd0c60/replication_gt
for extension in svg png; do
  curl -fsSL "$source_url/gid1-11.$extension" -o "/tmp/ferrodesign-figma-reference/gid1-11.$extension"
done
FERRODESIGN_FIGMA_REFERENCE=/tmp/ferrodesign-figma-reference \
FERRODESIGN_SCENARIOS=figma-artwork npm run test:browser
```

The scenario verifies both source hashes. Figma’s 375 × 812 document is exported at 2×. To produce the same 750 × 1624 pixels in FerroDesign, the runner doubles the SVG’s outer width and height while preserving the original viewBox, path data, transforms and clipping. It imports the SVG through the editor’s file picker, checks the editable node types and frame dimensions, then downloads the selected frame through **Export PNG**. The Figma PNG is read only by the comparison process.

Each renderer writes its native PNG, a JSON measurement and a difference image under `browser-report/`. The measurement counts exact RGBA matches, pixels within three channel values, pixels with a channel difference above sixteen, and mean absolute error over all four channels. A second set of counts excludes the two flat background colors in the reference, white and `#EAF2FF`. White in the difference image represents a maximum channel difference of 64 or more. The measurement covers this exported artwork, whose SVG represents lettering as paths. Editor chrome, editable text shaping and interactions require separate checks.

The baseline at `1a9ea40` produced the same export from both browser modes. Thresholds allow modest variation in antialiased edges. All four must pass. The runner writes the actual image, difference image and measurements before reporting a threshold failure.

| Region | Baseline within 3/255 on every RGBA channel | Minimum | Baseline mean absolute channel error | Maximum |
| --- | ---: | ---: | ---: | ---: |
| Whole 750 × 1624 frame | 99.300% | 99.1% | 0.0582 | 0.09 |
| 92,016 detail pixels | 91.133% | 90.5% | 0.7421 | 1.00 |

The source SVG encodes the button blue as `#006FFD`. The Figma PNG has `#006FFC` at 61,191 pixels that FerroDesign paints with the SVG value. Those pixels explain most of the gap between exact matches (93.842% overall and 19.404% on detail) and the within-three measurement.
