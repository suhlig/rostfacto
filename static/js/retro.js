// Retro board entry point. The board's behavior is split into focused ES
// modules; importing them here runs their top-level wiring. They communicate
// through CustomEvents on <body> (sse:*), so load order does not matter:
// every module has registered its listeners before any async event arrives.
//
//   sync.js         — the single EventSource, event dedup, card/action-item
//                     sync, and the sse:* events other modules react to
//   timer.js        — server-authoritative highlight countdown
//   participants.js — presence roster, ready toggle, and the panel UI
//   action-items.js — dated column grouping
//   shortcuts.js    — keyboard shortcuts
//   ui.js           — inline-handler replacements (click guards, form reset)

import './sync.js';
import './timer.js';
import './participants.js';
import './action-items.js';
import './shortcuts.js';
import './ui.js';
