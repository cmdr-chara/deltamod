const { defineConfig } = require("vite");
const { stageFrontend } = require("./scripts/stage-frontend");

module.exports = defineConfig({
  base: "./",
  plugins: [{
    name: "stage-desktop-frontend",
    closeBundle() { stageFrontend(__dirname); },
  }],
  build: {
    outDir: "web/boot",
    emptyOutDir: true,
    cssCodeSplit: false,
    lib: {
      entry: "web/boot-native-entry.js",
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
