// hooks.mjs — module resolve hook registered by export.mjs (node 22 `module.register`).
// The prototype's models.js / hunter.js import the bare specifier 'three' (served by an importmap in the browser);
// here it is redirected to the copy installed in tools/export/node_modules so prototype/ is never touched.
import { fileURLToPath, pathToFileURL } from 'node:url';
import path from 'node:path';

const here = path.dirname(fileURLToPath(import.meta.url));
const THREE_URL = pathToFileURL(path.join(here, 'node_modules', 'three', 'build', 'three.module.js')).href;

export async function resolve(specifier, context, nextResolve) {
  if (specifier === 'three') return { url: THREE_URL, shortCircuit: true };
  return nextResolve(specifier, context);
}
