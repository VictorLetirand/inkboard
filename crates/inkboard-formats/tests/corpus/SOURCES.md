# Test corpus: sources

Real files used by `tests/corpus.rs` and `tests/random_input.rs`. No network access happens in `cargo test`: the files were downloaded once, by hand, with `curl` from a URL pinned to a commit SHA, and are committed as is.

The license is the one GitHub reports for the whole repository (`gh api repos/<owner>/<repo>`, SPDX id) at download time; only MIT, Apache-2.0 and CC0-1.0 are accepted. File-level provenance was not audited. If an author asks for removal, delete the file and its line here.

Mermaid files are the fenced `mermaid` code blocks of the documentation pages, extracted verbatim (block number counted from 0 in the page, `mermaid-example` and `mermaid` fences together). `gantt` and `pie` are not supported by Inkboard: they must give an error.

## Mermaid (mermaid-js/mermaid, MIT, commit `81ad8076973d28bdbcb6dd9fab8bf1674e5380ea`)

| File | Source page and block | SHA-256 |
|---|---|---|
| `mermaid/flowchart_006.mmd` | [flowchart.md](https://github.com/mermaid-js/mermaid/blob/81ad8076973d28bdbcb6dd9fab8bf1674e5380ea/packages/mermaid/src/docs/syntax/flowchart.md) block 6 | `db4aa3083cae19b2161f65a3286ca21bd0af2272991be4d9b363418ce69e51ed` |
| `mermaid/flowchart_007.mmd` | [flowchart.md](https://github.com/mermaid-js/mermaid/blob/81ad8076973d28bdbcb6dd9fab8bf1674e5380ea/packages/mermaid/src/docs/syntax/flowchart.md) block 7 | `80b86f15a9c2e222a8366525c34dafb71e4ed8f6cdb146c66a55c3981a79d62b` |
| `mermaid/flowchart_092.mmd` | [flowchart.md](https://github.com/mermaid-js/mermaid/blob/81ad8076973d28bdbcb6dd9fab8bf1674e5380ea/packages/mermaid/src/docs/syntax/flowchart.md) block 92 | `49d93245842947c38b3169be332e9d9e4cf387b701c14d3f2f0dafc65103c206` |
| `mermaid/flowchart_096.mmd` | [flowchart.md](https://github.com/mermaid-js/mermaid/blob/81ad8076973d28bdbcb6dd9fab8bf1674e5380ea/packages/mermaid/src/docs/syntax/flowchart.md) block 96 | `b9675066f5fb1cb73c137c788366fc5bcd334cccfbbbee56b2fbb505ab782fd3` |
| `mermaid/flowchart_100.mmd` | [flowchart.md](https://github.com/mermaid-js/mermaid/blob/81ad8076973d28bdbcb6dd9fab8bf1674e5380ea/packages/mermaid/src/docs/syntax/flowchart.md) block 100 | `4b7f53d8caa034dd5818133bab4c99c38ed717201fc6ab1d30780faf50cd93ef` |
| `mermaid/gantt_000.mmd` | [gantt.md](https://github.com/mermaid-js/mermaid/blob/81ad8076973d28bdbcb6dd9fab8bf1674e5380ea/packages/mermaid/src/docs/syntax/gantt.md) block 0 | `9994b276372df6ec8dece5afd5a93326bf86e01f4e6574ed3e6c1fb052805144` |
| `mermaid/gantt_001.mmd` | [gantt.md](https://github.com/mermaid-js/mermaid/blob/81ad8076973d28bdbcb6dd9fab8bf1674e5380ea/packages/mermaid/src/docs/syntax/gantt.md) block 1 | `b854a6f49b60d20009c5c1b080f95308caa3b824add35a16afa3376699d40974` |
| `mermaid/pie_000.mmd` | [pie.md](https://github.com/mermaid-js/mermaid/blob/81ad8076973d28bdbcb6dd9fab8bf1674e5380ea/packages/mermaid/src/docs/syntax/pie.md) block 0 | `f41484cd94dd84837056db4c681ce6119f533b643bed745c58116553acdebf27` |
| `mermaid/pie_001.mmd` | [pie.md](https://github.com/mermaid-js/mermaid/blob/81ad8076973d28bdbcb6dd9fab8bf1674e5380ea/packages/mermaid/src/docs/syntax/pie.md) block 1 | `0618966cc9f61ec49975b0a1b5a9c0e3332461984a633de203a50897cc7b3bfc` |
| `mermaid/sequenceDiagram_001.mmd` | [sequenceDiagram.md](https://github.com/mermaid-js/mermaid/blob/81ad8076973d28bdbcb6dd9fab8bf1674e5380ea/packages/mermaid/src/docs/syntax/sequenceDiagram.md) block 1 | `b46ca8f030ae22c724720f00d238e528d416593c51d52a6e4f22d99b99f93b2d` |
| `mermaid/sequenceDiagram_015.mmd` | [sequenceDiagram.md](https://github.com/mermaid-js/mermaid/blob/81ad8076973d28bdbcb6dd9fab8bf1674e5380ea/packages/mermaid/src/docs/syntax/sequenceDiagram.md) block 15 | `5fa9f1e6f236b76b71424993a30eac3e12fa3c6b6f336ee7bc9d028c44914ccb` |
| `mermaid/sequenceDiagram_026.mmd` | [sequenceDiagram.md](https://github.com/mermaid-js/mermaid/blob/81ad8076973d28bdbcb6dd9fab8bf1674e5380ea/packages/mermaid/src/docs/syntax/sequenceDiagram.md) block 26 | `5ba07f36cdf08b28445a16fd8f24d05afbf5cd349679847aab70f7fddf476084` |
| `mermaid/stateDiagram_002.mmd` | [stateDiagram.md](https://github.com/mermaid-js/mermaid/blob/81ad8076973d28bdbcb6dd9fab8bf1674e5380ea/packages/mermaid/src/docs/syntax/stateDiagram.md) block 2 | `1e1b107393bac8aced893b21b2192b6ac3e7f993a5cbed5afe3c4aef8e488b0c` |
| `mermaid/stateDiagram_010.mmd` | [stateDiagram.md](https://github.com/mermaid-js/mermaid/blob/81ad8076973d28bdbcb6dd9fab8bf1674e5380ea/packages/mermaid/src/docs/syntax/stateDiagram.md) block 10 | `885c8b1b173cbaf7f8bc07d0ca01f5ab8e132e23da6339a0de2d78f390154622` |
| `mermaid/stateDiagram_013.mmd` | [stateDiagram.md](https://github.com/mermaid-js/mermaid/blob/81ad8076973d28bdbcb6dd9fab8bf1674e5380ea/packages/mermaid/src/docs/syntax/stateDiagram.md) block 13 | `8b4cf7c3adaa9c5f03a2cd872d4df54f7ae0f605807d79a020d5f3063153369d` |

## draw.io

| File | URL (pinned) | License | SHA-256 |
|---|---|---|---|
| `drawio/KamalRathnayake__17.drawio` | https://raw.githubusercontent.com/KamalRathnayake/MeetKamalToday./9b8462c6fa161728c4897a85415e437f24f9952f/17.drawio | MIT | `332492268ed38ac136ddbd863874359b9669f238149cd5b443edd4b040ef1b98` |
| `drawio/BreakingY__MCP.drawio` | https://raw.githubusercontent.com/BreakingY/Media-Cognition-Pipeline/110c5e88f3c4069eb5bb3bfab9209f3e1bc17742/MCP.drawio | MIT | `1856eaa48a397ceab75ad08e65dbbec5e4fa49808c351b0010602a842720562e` |
| `drawio/flying-dice__c4.drawio` | https://raw.githubusercontent.com/flying-dice/dcs-fiddle/23ecce019341ecca7a60c1ac550767b544642ebb/c4.drawio | MIT | `7184a48ebcd187a9952116759ef23bf5f40e864bb27c622dde3ca046dfe622e0` |
| `drawio/kubegems__ms.drawio` | https://raw.githubusercontent.com/kubegems/design/182b256014bf259b5d2219abd32591f74057b153/ms.drawio | Apache-2.0 | `2fc4828c772e6ffbb18bd8d8c9efab6a8dda852cdf1c98fa596fe49403d0f652` |
| `drawio/project-talan__ii.drawio` | https://raw.githubusercontent.com/project-talan/tln-clouds/2ac66333c46a0c70b8a01873438aeb4b733ba858/ii.drawio | MIT | `51abb1109c6daa42e4ae1713030a33e79829c47817bddc7d6473a565e6f2ea58` |
| `drawio/KIRAN-KUMAR-K3__er.drawio` | https://raw.githubusercontent.com/KIRAN-KUMAR-K3/railway-administration/02b4a91ede8cce7c1d4bb3cb90212be80d857023/er.drawio | MIT | `da3a6e4a8933f629bfe79b31f190b5335899e71bb5a3aa762d529202e6050809` |
| `drawio/nogibjj__p3.drawio` | https://raw.githubusercontent.com/nogibjj/Project3_Kashaf/be613e0b3128ccd1777a6a19f53fbc09c86b640c/p3.drawio | CC0-1.0 | `de3a13c4e5401d8b82b8e55f67ebe14586fac2800bfdc67445dd0984550ba8f9` |
| `drawio/chen-tianhao__EG.drawio` | https://raw.githubusercontent.com/chen-tianhao/PathMover/b3f32c57fe1c0e91dc605fd4d502267048fefd49/EG.drawio | MIT | `245d690a7df59c89eac252de7cc842a6a6e84be12ef73f3b0af66bbd7fe6be9f` |

## Excalidraw

| File | URL (pinned) | License | SHA-256 |
|---|---|---|---|
| `excalidraw/JonRC__sample.excalidraw` | https://raw.githubusercontent.com/JonRC/excalirender/379c9566b4e0f9a3bf18710eb5e6f87228c5c9e7/sample.excalidraw | MIT | `9b36ba428f48d2be3c4930d8e66e23d27682e6281aa9c181ed1f442286c10c5a` |
| `excalidraw/swiftlysingh__example.excalidraw` | https://raw.githubusercontent.com/swiftlysingh/excalidraw-cli/a8172f5fbbf1d45cc7a28dcc7850830cf90cdc09/example.excalidraw | MIT | `2ba974e1d31cf02003f81e1f520b76989b038fad9519c9afab4c2227d6daeef9` |
| `excalidraw/ralsina__kvm.excalidraw` | https://raw.githubusercontent.com/ralsina/kv/4c82557e53aef0078e8ba71723df6bedd28fc905/kvm.excalidraw | MIT | `b58b48dfa181d1c27781365ee52ade3cd0259297e7c18dd78b1ae320b6f66b26` |
| `excalidraw/aurorascharff__jokes.excalidraw` | https://raw.githubusercontent.com/aurorascharff/next15-remix-jokes-rebuild/073e7a821bef489d095562bce8e90f271b480262/jokes.excalidraw | MIT | `0e9dac69813074325a37fca776f6e350e30364ef77f1e100aefa64120773879a` |
| `excalidraw/josemiguel-alvarez__logo.excalidraw` | https://raw.githubusercontent.com/josemiguel-alvarez/simple-remix-blog/3627b57c41dd7bc0a06fbc28eff50c8cd5104473/logo.excalidraw | MIT | `927628e61306dd03dc526402dd119d20d57207966882b569d3dfb6c8ab857914` |
| `excalidraw/Bryan-Herrera-DEV__a.excalidraw` | https://raw.githubusercontent.com/Bryan-Herrera-DEV/Algorithms-Typescript/86fba1b8cefd37919117bfab8a302ea5196e64d0/a.excalidraw | MIT | `452452bed2ac0b5308c948aaa03dbaf35b5eb2281e15921a095c8a8cead5b8a7` |
| `excalidraw/parmsam__template.excalidraw` | https://raw.githubusercontent.com/parmsam/quarto-excalidraw/fb50b1f61e2421d8e873634bf0ca31f11419b5c8/template.excalidraw | MIT | `c17e1bcc23fd4640b485b576b56280373bf003622e1f9ab4f8c9f2a2527dad0d` |
| `excalidraw/abdheshnayak__arch.excalidraw` | https://raw.githubusercontent.com/abdheshnayak/inkube/8bceaef4b0db402bf0a260e621ac5885a5206304/arch.excalidraw | Apache-2.0 | `cfdaa396b80f00fc4ad269f361f540faaf3737c5ba1507905343054dbbd07954` |
| `excalidraw/renatus-cartesius__arch.excalidraw` | https://raw.githubusercontent.com/renatus-cartesius/reconswarm/9017be04d0b893bce22b5cba05848b2b901cfec7/arch.excalidraw | MIT | `c7a7f601c8b8b5011ed3eb91c292f83b17253310b91686faeedd76891d397821` |
| `excalidraw/longsurf-ai__pine.excalidraw` | https://raw.githubusercontent.com/longsurf-ai/tea/137295ce5f4c27d9c658bcf94d2d34d1b40968f0/pine.excalidraw | MIT | `093eb81ec805c21820f426a91508c8686470e9cd4529bdeff0423da8c0cd714e` |
| `excalidraw/Milo123459__bento.excalidraw` | https://raw.githubusercontent.com/Milo123459/bento/52ca4db622e4bd5e5ba854bbf8cda5c21972c3ba/bento.excalidraw | MIT | `805aee96570b44ae2a504ad042cde7a81d1577b716e912f4af4e81c9f0001212` |

## JSON Canvas (Obsidian)

| File | URL (pinned) | License | SHA-256 |
|---|---|---|---|
| `canvas/obsidianmd__sample.canvas` | https://raw.githubusercontent.com/obsidianmd/jsoncanvas/456f843cb293df4f4ab1763e22ccb46a80b307c8/sample.canvas | MIT | `ad089f3cbf255924d912a1e6fdc29d3414d9c7264b2c6c878e38680c3f33a7ce` |
| `canvas/subtleGradient__appify-ui.canvas` | https://raw.githubusercontent.com/subtleGradient/Appify-UI/bb5fdc142ad312b50d797cbfd020e676d5345094/appify-ui.canvas | MIT | `9a89ba0713492d2092769863e0f7b52cadf0acb1a41e0d4f5cfe69fff49dedbf` |
| `canvas/otaleghani__Canvas.canvas` | https://raw.githubusercontent.com/otaleghani/kiln/95372f0f5521588b93fafc82f11dd23502e44f08/docs/Canvas.canvas | MIT | `c9c8733637a5013a9bf70d1dc9475e7d50a13bf8838d9aacdb949e780783e39e` |
| `canvas/k0rdent__otel.canvas` | https://raw.githubusercontent.com/k0rdent/kof/1f0484e744c875db4a24c38f91f86f88bdb3e1a8/docs/otel.canvas | Apache-2.0 | `5eb73049d3e14724601e32cb4013c24c66a00dd999e42a4b59d81f406f82715b` |
| `canvas/chrisgrieser__alt-file.canvas` | https://raw.githubusercontent.com/chrisgrieser/grappling-hook/e8b1908f911884ea0d11fed0437f1853c369a3e4/docs/alt-file.canvas | MIT | `30efd8fff9380dd3b1cf956622b3fd755386c8687748b77e970a43514556ba16` |
| `canvas/lout33__example1.canvas` | https://raw.githubusercontent.com/lout33/infinite_canvas_vscode/4f8cbabc02c072faeef8590b00d238ebca8ff087/docs/example1.canvas | MIT | `c88e5ba11fbff08c34dae55ac6e5a8c0e90c16f6d1442132b3498898119b0efb` |
| `canvas/FitHappensML__example.canvas` | https://raw.githubusercontent.com/FitHappensML/canvas-links-web-viewer/ae1c85a4941e1dfec9a9661136be591f51d4f663/example/example.canvas | MIT | `1bb7c225d5f6d1ae652b3e5d545f4774a5e9d41abce3df4e07036e25b42abb81` |
| `canvas/lovettbarron__example.canvas` | https://raw.githubusercontent.com/lovettbarron/rehype-jsoncanvas/8de76923cea13f2052c551d0d0f77c792cbc9c73/example/example.canvas | MIT | `a44c74ba45fb34262de23e460d732cea418ac969a828feb9fa6024bbc5c44f9b` |
| `canvas/juanklagos__board.canvas` | https://raw.githubusercontent.com/juanklagos/spec-driven-development-template/939649d7d482026ede829547a0c0fe413fe86a6f/specs/board.canvas | MIT | `ed199c75fada6cd68ad589ae634855cf90634d047eb116002d95395c3126a23b` |
| `canvas/enetheru__QuestGraph.canvas` | https://raw.githubusercontent.com/enetheru/godot-flatbuffers/798c58a600a6331e6877f543e5f777dee4f6d578/Notes/QuestGraph.canvas | MIT | `6f37ce4a0bf1d052767dfdb21a370939c92861b654783e3402923ef86baa7602` |
| `canvas/qiyangdev__demo.canvas` | https://raw.githubusercontent.com/qiyangdev/vaultpress/8946937f324a8335782519c00b054aa81d2fcfc2/public/canvas/demo.canvas | MIT | `564871c49dde4e4c8035d0394304f76a6572e16fc0cee10c29c5c703f2566e4e` |
