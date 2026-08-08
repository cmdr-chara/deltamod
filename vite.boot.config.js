const { defineConfig } = require("vite");

module.exports = defineConfig({
  base: "./",
  // React's production branch is browser-safe; without this replacement the
  // library build leaves a Node `process` reference in the deltapack script.
  define: {
    "process.env.NODE_ENV": JSON.stringify("production"),
  },
  build: {
    outDir: "web/boot",
    emptyOutDir: true,
    cssCodeSplit: false,
    lib: {
      entry: "web/boot-entry.tsx",
      name: "DeltamodBootBundle",
      formats: ["iife"],
      fileName: () => "deltamod-boot.js",
      cssFileName: "deltamod-boot",
    },
    rollupOptions: {
      output: {
        assetFileNames: (assetInfo) => (
          assetInfo.name?.endsWith(".css")
            ? "deltamod-boot.css"
            : "assets/[name]-[hash][extname]"
        ),
      },
    },
    sourcemap: false,
    minify: true,
  },
});
