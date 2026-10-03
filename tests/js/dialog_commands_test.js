// Unit tests for the Invoker Commands fallback in dialog-commands.js.
// Run with `deno test tests/js/`.

import { installDialogCommandFallback } from '../../static/js/dialog-commands.js';

function fakeWindow(supportsInvokerCommands) {
  const prototype = {};
  if (supportsInvokerCommands) prototype.command = 'show-modal';
  return { HTMLButtonElement: { prototype } };
}

function fakeDocument() {
  const listeners = {};
  const dialogs = new Map();
  return {
    dialogs,
    addEventListener(type, handler) {
      listeners[type] = handler;
    },
    getElementById(id) {
      return dialogs.get(id) ?? null;
    },
    hasClickListener() {
      return typeof listeners.click === 'function';
    },
    click(target) {
      listeners.click({ target });
    },
  };
}

function fakeDialog(open) {
  return {
    open,
    showModalCalls: 0,
    closeCalls: 0,
    showModal() {
      this.showModalCalls++;
      this.open = true;
    },
    close() {
      this.closeCalls++;
      this.open = false;
    },
  };
}

function fakeButton(command, commandfor) {
  return {
    getAttribute(name) {
      if (name === 'command') return command;
      if (name === 'commandfor') return commandfor;
      return null;
    },
  };
}

function fakeClickTarget(button) {
  return {
    closest(selector) {
      return selector === '[command][commandfor]' ? button : null;
    },
  };
}

function assert(condition, message) {
  if (!condition) throw new Error(message);
}

Deno.test('installs a click listener when Invoker Commands are unsupported', () => {
  const doc = fakeDocument();
  installDialogCommandFallback(fakeWindow(false), doc);
  assert(doc.hasClickListener(), 'expected a click listener to be installed');
});

Deno.test('does nothing when Invoker Commands are natively supported', () => {
  const doc = fakeDocument();
  installDialogCommandFallback(fakeWindow(true), doc);
  assert(!doc.hasClickListener(), 'expected no click listener with native support');
});

Deno.test('show-modal opens the target dialog', () => {
  const doc = fakeDocument();
  const dialog = fakeDialog(false);
  doc.dialogs.set('modal', dialog);
  installDialogCommandFallback(fakeWindow(false), doc);
  doc.click(fakeClickTarget(fakeButton('show-modal', 'modal')));
  assert(dialog.showModalCalls === 1, 'expected showModal() to be called once');
});

Deno.test('show-modal does not re-open an already open dialog', () => {
  const doc = fakeDocument();
  const dialog = fakeDialog(true);
  doc.dialogs.set('modal', dialog);
  installDialogCommandFallback(fakeWindow(false), doc);
  doc.click(fakeClickTarget(fakeButton('show-modal', 'modal')));
  assert(dialog.showModalCalls === 0, 'expected showModal() not to be called');
});

Deno.test('close closes the target dialog', () => {
  const doc = fakeDocument();
  const dialog = fakeDialog(true);
  doc.dialogs.set('modal', dialog);
  installDialogCommandFallback(fakeWindow(false), doc);
  doc.click(fakeClickTarget(fakeButton('close', 'modal')));
  assert(dialog.closeCalls === 1, 'expected close() to be called once');
});

Deno.test('ignores clicks that are not on a command button', () => {
  const doc = fakeDocument();
  installDialogCommandFallback(fakeWindow(false), doc);
  doc.click({ closest: () => null });
});

Deno.test('ignores a command whose target dialog is missing', () => {
  const doc = fakeDocument();
  installDialogCommandFallback(fakeWindow(false), doc);
  doc.click(fakeClickTarget(fakeButton('show-modal', 'missing')));
});
