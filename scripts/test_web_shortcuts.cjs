// Run with node --test scripts/test_web_shortcuts.cjs; no app build required.
const {test} = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');

const html = fs.readFileSync(path.join(__dirname, '../index.html'), 'utf8');
const source = html.match(/<script id="workspace-shortcuts">([\s\S]*?)<\/script>/)[1];
const canvas = {};
const textInput = {};
let handler;
const workspace = {
    addEventListener(type, callback, options) {
        assert.equal(type, 'keydown');
        assert.equal(options.capture, true);
        assert.equal(options.passive, false);
        handler = callback;
    },
};
vm.runInNewContext(source, {document: {getElementById: id => ({
    'ketchup-workspace': workspace, 'ketchup-canvas': canvas,
})[id]}});

function cancelled(key, options = {}) {
    const event = {
        key, code: '', target: canvas, ctrlKey: false, metaKey: false,
        isComposing: false, getModifierState: () => false,
        preventDefault() { this.defaultPrevented = true; },
        // Cancelling browser defaults must not stop delivery to eframe.
        stopPropagation() { assert.fail('Shortcut did not reach eframe'); },
        stopImmediatePropagation() { assert.fail('Shortcut did not reach eframe'); },
        ...options,
    };
    handler(event);
    return !!event.defaultPrevented;
}

test('canvas and eframe sibling input share the capture listener', () => {
    assert.match(html, /<div id="ketchup-workspace">\s*<canvas[^>]*id="ketchup-canvas"[^>]*><\/canvas>\s*<\/div>/);
});

test('app commands cancel browser defaults on canvas and in text fields', () => {
    for (const target of [canvas, textInput]) {
        for (const modifier of ['ctrlKey', 'metaKey']) {
            for (const key of ['a', 'f', 'n', 'o', 's', 'y', 'z', 'A', 'F', 'N', 'O', 'S', 'Y', 'Z']) {
                for (const shiftKey of [false, true]) {
                    assert(cancelled(key, {target, [modifier]: true, shiftKey}), `${modifier}+${key}`);
                }
            }
        }
    }
    assert(cancelled('z', {ctrlKey: true, repeat: true}));
});

test('clipboard commands still generate native clipboard events', () => {
    for (const target of [canvas, textInput]) {
        for (const modifier of ['ctrlKey', 'metaKey']) {
            for (const key of ['c', 'x', 'v', 'C', 'X', 'V']) {
                for (const shiftKey of [false, true]) {
                    assert(!cancelled(key, {target, [modifier]: true, shiftKey}));
                }
            }
        }
    }
});

test('tools and navigation cancel defaults without breaking text entry', () => {
    for (const key of [' ', ...'ab cdefgilmopqrst'.replaceAll(' ', ''),
        'ArrowDown', 'ArrowLeft', 'ArrowRight', 'ArrowUp', 'Backspace',
        'Delete', 'Enter', 'Tab']) {
        assert(cancelled(key), key);
        assert(!cancelled(key, {target: textInput}), `text input: ${key}`);
    }
    for (const key of ['F1', 'Escape']) {
        assert(cancelled(key));
        assert(cancelled(key, {target: textInput}));
    }
});

test('unassigned browser shortcuts and ordinary typing remain available', () => {
    for (const modifier of ['ctrlKey', 'metaKey']) {
        for (const key of ['l', 'r', 't', 'w', '+', '-', '0']) {
            assert(!cancelled(key, {[modifier]: true}), `${modifier}+${key}`);
        }
    }
    for (const key of ['F5', 'F12', '1', '.', ',', 'u']) assert(!cancelled(key));
});

test('non-Latin layouts use the same physical fallback as eframe', () => {
    assert(cancelled('я', {code: 'KeyZ', ctrlKey: true}));
    assert(cancelled('д', {code: 'KeyL'}));
    assert(!cancelled('y', {code: 'KeyZ', ctrlKey: true, getModifierState: key => key === 'AltGraph'}));
});

test('IME composition is not intercepted', () => {
    assert(!cancelled('z', {ctrlKey: true, isComposing: true}));
    assert(!cancelled('Enter', {isComposing: true}));
});
