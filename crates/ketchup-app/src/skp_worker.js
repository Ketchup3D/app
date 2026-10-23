// Function body used by the WASM host. Keep lifecycle cleanup on every exit.
return new Promise((resolve, reject) => {
    const body = source + `\nonmessage = event => {
        try { postMessage({model: ketchupImport(new Uint8Array(event.data))}); }
        catch (error) { postMessage({error: error.message || String(error)}); }
    };`;
    const url = URL.createObjectURL(new Blob([body], {type: 'text/javascript'}));
    let worker;
    const cleanup = () => { if (worker) worker.terminate(); URL.revokeObjectURL(url); };
    try {
        worker = new Worker(url);
        worker.onmessage = event => {
            cleanup();
            if (event.data.error) reject(new Error(event.data.error));
            else resolve(event.data.model);
        };
        worker.onerror = event => { cleanup(); reject(new Error(event.message || 'SketchUp worker failed')); };
        worker.onmessageerror = () => { cleanup(); reject(new Error('Could not receive the SketchUp model')); };
        worker.postMessage(bytes.buffer, [bytes.buffer]);
    } catch (error) { cleanup(); reject(error); }
});
