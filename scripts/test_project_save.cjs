// Run with node --test scripts/test_project_save.cjs; no app build required.
const {test} = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');

const save = new Function('window', 'name', 'contents', fs.readFileSync(
    path.join(__dirname, '../crates/ketchup-app/src/project_save.js'), 'utf8',
));

test('save prompts immediately and reports the chosen name only after committing the file', async () => {
    const events = [];
    let finishClose;
    const closed = new Promise(resolve => { finishClose = resolve; });
    const window = {
        showSaveFilePicker(options) {
            events.push('picker');
            assert.equal(options.suggestedName, 'House.ketchup');
            assert.equal(options.id, 'ketchup-project-save');
            assert.deepEqual(options.types[0].accept, {'application/json': ['.ketchup']});
            return Promise.resolve({
                name: 'New house.ketchup',
                async createWritable() {
                    events.push('create');
                    return {
                        async write(contents) {
                            assert.equal(contents, '{"name":"House"}');
                            events.push('write');
                        },
                        async close() { events.push('close'); await closed; },
                        async abort() { assert.fail('Successful save must commit'); },
                    };
                },
            });
        },
    };
    const pending = save(window, 'House.ketchup', '{"name":"House"}');
    assert.deepEqual(events, ['picker'], 'Open within the user action, before yielding');
    let completed = false;
    pending.then(() => { completed = true; });
    await new Promise(resolve => setImmediate(resolve));
    assert.deepEqual(events, ['picker', 'create', 'write', 'close']);
    assert.equal(completed, false, 'Do not mark saved before close finishes');
    finishClose();
    assert.equal(await pending, 'New house.ketchup');
});

test('unsupported browsers request the existing download fallback synchronously', () => {
    assert.equal(save({}, 'House.ketchup', '{}'), null);
});

test('cancelling the picker completes as cancellation instead of requesting a download', async () => {
    const pending = save({
        async showSaveFilePicker() { throw {name: 'AbortError'}; },
    }, 'House.ketchup', '{}');
    assert(pending instanceof Promise, 'An opened picker owns completion; never fall back');
    assert.equal(await pending, null);
});

test('picker and permission failures propagate instead of requesting a download', async () => {
    for (const synchronous of [false, true]) {
        const error = new Error('Permission denied');
        const pending = save({
            showSaveFilePicker() {
                if (synchronous) throw error;
                return Promise.reject(error);
            },
        }, 'House.ketchup', '{}');
        await assert.rejects(pending, candidate => candidate === error);
    }
    const error = new Error('File is read-only');
    await assert.rejects(save({
        async showSaveFilePicker() {
            return {async createWritable() { throw error; }};
        },
    }, 'House.ketchup', '{}'), candidate => candidate === error);
});

test('write and commit failures abort the stream and never report success', async () => {
    for (const failure of ['write', 'close']) {
        const events = [];
        // Even an AbortError during writing is an error, not picker cancellation.
        const error = Object.assign(new Error('Disk write failed'), {name: 'AbortError'});
        const pending = save({
            async showSaveFilePicker() {
                return {
                    name: 'House.ketchup',
                    async createWritable() {
                        return Object.fromEntries(['write', 'close', 'abort'].map(action => [
                            action, async () => {
                                events.push(action);
                                if (action === failure) throw error;
                                if (action === 'abort') throw new Error('Already closed');
                            },
                        ]));
                    },
                };
            },
        }, 'House.ketchup', '{}');
        await assert.rejects(pending, candidate => candidate === error);
        assert.deepEqual(events, failure === 'write' ? ['write', 'abort'] : ['write', 'close', 'abort']);
    }
});
