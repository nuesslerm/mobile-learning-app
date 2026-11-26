# Opening files from a paired chat

On the rca iPhone 18 Pro Max, installed Orca 0.0.68 (1) opened Files from the chat’s More actions menu. Its header showed the checkout name and a back button. The reference host remained on “Loading files…” during this capture. The populated tree and reader below were verified on Expo Go and the dark mobile web export.

| Step | Installed Orca | Expo Go |
| --- | --- | --- |
| Open More actions | [Menu](orca-menu.png) | [Menu](expo-menu.png) |
| Open File explorer | [Files](orca-files.png) | [Files](expo-files.png) |
| Expand a folder and open text | | [Tree](expo-tree.png) · [Preview](expo-preview.png) |

The Expo session reused the paired chat’s authenticated T3 socket. Inside the checkout it expanded orca-file-preview, opened hello.md, read a line containing 🌿 and returned through the expanded tree to the same chat. The tree before and after reading the file matched across all 3,541,560 pixels below the status bar. The authenticated thread projection still had its two original messages and model. A route flow also verified draft preservation.

Across the Files header band (x=0–1319, y=185–348), 6,370 of 216,480 pixels differed between the two clients. The back chevron occupied the same bounding box. The Expo title began one pixel higher and the subtitle two pixels higher. Expo currently exposes File explorer in the menu. Orca also offered Quick commands and Text size.

## Runbooks and fixtures

The [Expo runbook](expo-files.lua) starts on the paired desktop’s checkout list in the recorded rca layout. The [web runbook](web.lua) starts inside the paired native-fixture chat. Both use a disposable folder named orca-file-preview containing binary.bin, hello.md and large.txt. The first file contains a NUL byte. The text file contains the three lines below, including its final newline. The large file consists of 1,048,575 ASCII a characters followed by 🌿 and a newline. Add the fixture only to a disposable checkout and pass a writable capture directory as out.

```text
Orca file preview

A leaf 🌿 from the paired checkout.
```

The live projects.readFile response preserved all 57 bytes of hello.md. For large.txt the server returned the 1,048,575 complete UTF-8 bytes in its preview and reported byteLength=1048580 and truncated=true. Binary input produced ProjectReadFileError with failure=binary_file. An optional absent file produced exists=false. The executor integration test traced worktree.list plus file.readChunk for a successful read, without a separate file stat.

[Reference runbook](orca-files.lua) · [Expo recording](expo.mp4) · [Dark web preview](web-preview.png) · [Web recording](web.mp4) · [iOS EAS Update](https://expo.dev/accounts/nuesslerm/projects/mla-tamagui/updates/d9f674f7-45d7-4711-bce2-f146bd54d0c0)
