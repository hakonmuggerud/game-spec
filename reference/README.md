# reference/

Frozen history of the port. Nothing in here is built, tested or edited any more.

- `prototype/` — the Three.js prototype (one `index.html` + ES modules in `src/`, Three.js 0.160
  from a CDN) that the Bevy port in [`../undercroft`](../undercroft) was verified against, frame for
  frame, with a scripted playthrough. It is the origin every ported item's doc comment cites
  (`reference/prototype/src/hunter.js` and so on) and the reason the numbers in the RON tables are
  what they are. Do not edit it: a change here no longer changes the game.

## Serving it for a side-by-side

The prototype still runs in a desktop browser (it needs internet for the CDN and WebGL + pointer lock):

```sh
python3 -m http.server 8765 --bind 0.0.0.0 --directory /home/agent/repos/game-spec
# http://<host>:8765/reference/prototype/index.html   (prototype)
# http://<host>:8765/undercroft/dist/                 (Bevy wasm build, after `trunk build`)
```

`undercroft/tools/qa/proto.mjs` drives it headlessly through `window.__game.actions` with the same
step list `UNDERCROFT_SCRIPT` takes on the Bevy side, which is how parity screenshots were made;
its README has the recipe.
