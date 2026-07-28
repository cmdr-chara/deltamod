const { contextBridge, ipcRenderer } = require('electron');

contextBridge.exposeInMainWorld('progressAPI', {
    onProgress(callback) {
        const listener = (_event, value) => callback(Number(value) || 0);
        ipcRenderer.on('progress', listener);
        return () => ipcRenderer.removeListener('progress', listener);
    }
});
