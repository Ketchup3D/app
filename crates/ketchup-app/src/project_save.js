// Called synchronously from the app's save action to retain user activation.
// null means this browser needs the download fallback; a Promise means the
// picker owns the operation, including cancellation and write failures.
if (typeof window.showSaveFilePicker !== 'function') return null;
return (async () => {
    let handle;
    try {
        handle = await window.showSaveFilePicker({
            id: 'ketchup-project-save',
            suggestedName: name,
            types: [{
                description: 'KetchUp project',
                accept: {'application/json': ['.ketchup']},
            }],
        });
    } catch (error) {
        if (error.name === 'AbortError') return null;
        throw error;
    }
    const writable = await handle.createWritable();
    try {
        await writable.write(contents);
        await writable.close();
    } catch (error) {
        // Do not commit a partial file if writing or closing fails.
        try { await writable.abort(); } catch {}
        throw error;
    }
    return handle.name;
})();
