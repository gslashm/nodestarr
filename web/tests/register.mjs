// Lets Node's test runner load the UI's TypeScript modules directly: Node
// strips the types, and this hook resolves extensionless relative imports
// ("./format") to their .ts files the way Vite does.
import { register } from 'node:module';

register('./resolve-ts.mjs', import.meta.url);
