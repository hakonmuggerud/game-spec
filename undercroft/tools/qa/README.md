# tools/qa — reference screenshots of the prototype

`proto.mjs` drives the Three.js prototype in headless Chromium (system `/usr/bin/chromium`,
SwiftShader WebGL) through `window.__game.actions`, so a Bevy screenshot from
`UNDERCROFT_SCRIPT` can be compared against the same moment in the reference.

```sh
cd tools/qa && npm install                       # playwright-core only, no browser download
python3 -m http.server 8765 --bind 0.0.0.0 --directory ../../..   # serves /prototype/
node proto.mjs "wait 1; __game.actions.begin(); wait 2; __game.actions.gotoZone('undercroft'); wait 2" out.png
```

Steps are `;`-separated: `wait N` or any JS expression evaluated in the page. The script prints
each expression's result and the final mode/player position.
