# Pairing on the RCA simulator

The pinned Orca client opened its pairing route with the existing host still paired. On the iPhone 18 Pro Max in light appearance, **Paste code instead** opened a drawer, focused the empty field, showed the iOS keyboard and disabled Save. Submitting an invalid invite dismissed the drawer. **Try Again** returned to the scanner introduction. The [RCA runbook](paste-and-retry.lua) and [recording](paste-and-retry.mp4) cover that round trip. See the [focused drawer](paste.png) and [invalid code](invalid.png).

Tapping **Continue** on a fresh permission state showed the [system camera prompt](permission.png). Denying access showed the [recovery screen](denied.png) with Open Settings and Paste code instead. The camera hardware path still needs verification on a phone.

## Native Expo comparison

On the same RCA simulator, the signed Expo build opened pairing from the connected workspace and ran the [invalid-code and retry journey](compare-expo.lua). The [Expo introduction](comparison/intro.png) matches [Orca](../unpaired/pair-scan.png) at every one of **3,762,045** compared pixels. Both captures are 1320 × 2868. Two masks cover the system clock (205,50 to 340,120) and the previous-app label (30,113 to 420,152). Without masks, 5,467 pixels differ. The Continue button uses the continuous corner curve from Orca’s themed stylesheet.

Compare the settled paste drawers in [Orca](paste-settled.png) and [Expo](comparison/paste-settled.png), the [Expo invalid invite](comparison/invalid.png), and the [Expo recording](comparison/paste-and-retry.mp4). Drawer motion, text field and keyboard rendering are still open. A second native run replaced the invalid input with a freshly issued RCA T3 pairing link and reached the saved workspace. Published captures only show the synthetic invalid input.
