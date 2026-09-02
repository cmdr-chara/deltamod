// Copyright © 2026 cmdr-chara
// Modified for Deltamod Community on 2026-07-29.
// Licensed under the EUPL 1.2.

(function exposeThemeSprites(globalScope) {
    const sourceCache = new Map();
    const resultCache = new Map();
    let activePalette = '';
    const SOUL_COLORS = Object.freeze([
        Object.freeze({ name: 'red', hex: '#FF0000', rgb: Object.freeze([255, 0, 0]) }),
        Object.freeze({ name: 'orange', hex: '#FCA600', rgb: Object.freeze([252, 166, 0]) }),
        Object.freeze({ name: 'yellow', hex: '#FFFF00', rgb: Object.freeze([255, 255, 0]) }),
        Object.freeze({ name: 'green', hex: '#00C000', rgb: Object.freeze([0, 192, 0]) }),
        Object.freeze({ name: 'light-blue', hex: '#42FCFF', rgb: Object.freeze([66, 252, 255]) }),
        Object.freeze({ name: 'blue', hex: '#003CFF', rgb: Object.freeze([0, 60, 255]) }),
        Object.freeze({ name: 'purple', hex: '#D535D9', rgb: Object.freeze([213, 53, 217]) })
    ]);
    const ACCENT_MAPS = Object.freeze({
        'main.png': {
            '145,74,59': 'deep',
            '185,122,87': 'shadow',
            '177,148,139': 'base',
            '202,179,150': 'light',
            '246,234,171': 'highlight'
        },
        'allmods.png': {
            '136,35,70': 'deep',
            '184,79,100': 'shadow',
            '255,84,86': 'base',
            '37,70,75': 'rightDeep',
            '0,91,130': 'rightShadow',
            '44,136,133': 'rightBase',
            '48,169,142': 'rightLight'
        },
        'options.png': {
            '46,41,61': 'deep',
            '65,57,85': 'shadow',
            '97,78,107': 'base'
        },
        'installmanager.png': {
            '54,28,32': 'deep',
            '75,53,61': 'shadow',
            '96,78,90': 'base',
            '131,98,135': 'light',
            '172,164,215': 'highlight',
            '18,99,188': 'rightShadow',
            '172,50,50': 'base'
        },
        'shop.png': {
            '0,51,121': 'deep',
            '0,133,176': 'shadow',
            '14,163,14': 'base',
            '0,191,255': 'highlight'
        },
        'collections.png': {
            '20,34,113': 'leftDeep',
            '27,45,143': 'leftShadow',
            '62,70,146': 'leftShadow',
            '34,54,167': 'leftBase',
            '45,70,210': 'leftLight',
            '94,106,214': 'leftHighlight',
            '24,59,6': 'deep',
            '39,90,13': 'shadow',
            '65,140,26': 'base',
            '99,206,44': 'light',
            '124,211,93': 'highlight',
            '84,12,76': 'rightDeep',
            '126,23,115': 'rightShadow',
            '140,26,128': 'rightBase',
            '163,33,149': 'rightLight',
            '206,44,188': 'rightLight',
            '211,93,195': 'rightHighlight'
        },
        'credits.png': {
            '44,136,133': 'base'
        }
    });

    function parseThemeColor(value) {
        const source = String(value || '').trim();
        const hexMatch = source.match(/^#([0-9a-f]{6})$/i);
        if (hexMatch) {
            return [0, 2, 4].map(offset =>
                Number.parseInt(hexMatch[1].slice(offset, offset + 2), 16)
            );
        }
        const match = source.match(
            /^rgb\(\s*(\d{1,3})\s*,\s*(\d{1,3})\s*,\s*(\d{1,3})\s*\)$/i
        );
        if (!match) return null;
        const color = match.slice(1).map(Number);
        return color.every(channel => channel >= 0 && channel <= 255) ? color : null;
    }

    function mixColor(from, to, amount) {
        return from.map((channel, index) =>
            Math.round(channel * (1 - amount) + to[index] * amount)
        );
    }

    function relativeLuminance(color) {
        const channels = color.map(channel => {
            const normalized = channel / 255;
            return normalized <= 0.04045
                ? normalized / 12.92
                : ((normalized + 0.055) / 1.055) ** 2.4;
        });
        return (0.2126 * channels[0]) + (0.7152 * channels[1]) + (0.0722 * channels[2]);
    }

    function contrastRatio(first, second) {
        const firstLuminance = relativeLuminance(first);
        const secondLuminance = relativeLuminance(second);
        return (Math.max(firstLuminance, secondLuminance) + 0.05)
            / (Math.min(firstLuminance, secondLuminance) + 0.05);
    }

    function readableInkColor(background) {
        const white = [255, 255, 255];
        const black = [0, 0, 0];
        return contrastRatio(background, white) >= contrastRatio(background, black)
            ? '#ffffff'
            : '#000000';
    }

    function controlPalette(color) {
        const hoverColor = mixColor(color, [255, 255, 255], 0.16);
        return {
            hoverColor,
            inkColor: readableInkColor(color),
            hoverInkColor: readableInkColor(hoverColor)
        };
    }

    function colorHue(color) {
        const normalized = color.map(channel => channel / 255);
        const maximum = Math.max(...normalized);
        const minimum = Math.min(...normalized);
        const delta = maximum - minimum;
        if (delta === 0) return null;

        let hue;
        if (maximum === normalized[0]) {
            hue = 60 * (((normalized[1] - normalized[2]) / delta) % 6);
        } else if (maximum === normalized[1]) {
            hue = 60 * ((normalized[2] - normalized[0]) / delta + 2);
        } else {
            hue = 60 * ((normalized[0] - normalized[1]) / delta + 4);
        }
        return hue < 0 ? hue + 360 : hue;
    }

    function canonicalSoulColor(color) {
        const hue = colorHue(color);
        if (hue === null) return [...SOUL_COLORS[0].rgb];

        const soulName = hue < 20 || hue >= 330 ? 'red'
            : hue < 50 ? 'orange'
                : hue < 90 ? 'yellow'
                    : hue < 165 ? 'green'
                        : hue < 205 ? 'light-blue'
                            : hue < 265 ? 'blue'
                                : 'purple';
        return [...SOUL_COLORS.find(soul => soul.name === soulName).rgb];
    }

    function rotateHue(color, degrees) {
        const normalized = color.map(channel => channel / 255);
        const maximum = Math.max(...normalized);
        const minimum = Math.min(...normalized);
        const delta = maximum - minimum;
        let hue = 0;
        if (delta !== 0) {
            if (maximum === normalized[0]) {
                hue = 60 * (((normalized[1] - normalized[2]) / delta) % 6);
            } else if (maximum === normalized[1]) {
                hue = 60 * ((normalized[2] - normalized[0]) / delta + 2);
            } else {
                hue = 60 * ((normalized[0] - normalized[1]) / delta + 4);
            }
        }
        if (hue < 0) hue += 360;
        const lightness = (maximum + minimum) / 2;
        const saturation = delta === 0
            ? 0
            : delta / (1 - Math.abs(2 * lightness - 1));
        const targetHue = ((hue + degrees) % 360 + 360) % 360;
        const chroma = (1 - Math.abs(2 * lightness - 1)) * saturation;
        const section = targetHue / 60;
        const intermediate = chroma * (1 - Math.abs((section % 2) - 1));
        const channels = section < 1 ? [chroma, intermediate, 0]
            : section < 2 ? [intermediate, chroma, 0]
                : section < 3 ? [0, chroma, intermediate]
                    : section < 4 ? [0, intermediate, chroma]
                        : section < 5 ? [intermediate, 0, chroma]
                            : [chroma, 0, intermediate];
        const offset = lightness - chroma / 2;
        return channels.map(channel => Math.round((channel + offset) * 255));
    }

    function paletteForColor(color) {
        const createScale = base => ({
            deep: mixColor([0, 0, 0], base, 0.42),
            shadow: mixColor([0, 0, 0], base, 0.68),
            base: [...base],
            light: mixColor(base, [255, 255, 255], 0.38),
            highlight: mixColor(base, [255, 255, 255], 0.62)
        });
        const center = createScale(color);
        const left = createScale(rotateHue(color, -32));
        const right = createScale(rotateHue(color, 32));
        return {
            ...center,
            leftDeep: left.deep,
            leftShadow: left.shadow,
            leftBase: left.base,
            leftLight: left.light,
            leftHighlight: left.highlight,
            rightDeep: right.deep,
            rightShadow: right.shadow,
            rightBase: right.base,
            rightLight: right.light,
            rightHighlight: right.highlight,
            soulBase: color,
            soulShadow: mixColor([0, 0, 0], color, 0.48),
            soulHighlight: mixColor(color, [255, 255, 255], 0.42)
        };
    }

    function buildPalette(color) {
        return paletteForColor(color);
    }

    function recolorPixels(source, color, mode, spriteName = '') {
        const output = new Uint8ClampedArray(source);
        const palette = buildPalette(canonicalSoulColor(color));
        const accentMap = ACCENT_MAPS[spriteName] || {};

        for (let index = 0; index < output.length; index += 4) {
            const red = source[index];
            const green = source[index + 1];
            const blue = source[index + 2];
            const alpha = source[index + 3];
            if (alpha === 0) continue;

            let target;
            if (mode === 'soul') {
                const isSoulPixel =
                    blue >= 100 &&
                    green >= 70 &&
                    blue > red * 1.7 &&
                    green > red * 1.7;
                if (!isSoulPixel) continue;

                const luminance = 0.2126 * red + 0.7152 * green + 0.0722 * blue;
                target = luminance < 100
                    ? palette.soulShadow
                    : luminance < 190
                        ? palette.soulBase
                        : palette.soulHighlight;
            } else {
                const paletteRole = accentMap[`${red},${green},${blue}`];
                if (!paletteRole) continue;
                target = palette[paletteRole];
            }

            output[index] = target[0];
            output[index + 1] = target[1];
            output[index + 2] = target[2];
        }

        return output;
    }

    function paletteTone(palette, luminance) {
        if (luminance < 65) return palette.deep;
        if (luminance < 105) return palette.shadow;
        if (luminance < 155) return palette.base;
        if (luminance < 205) return palette.light;
        return palette.highlight;
    }

    function recolorAppIconPixels(source, accentColor, soulColor) {
        const output = new Uint8ClampedArray(source);
        const accent = paletteForColor(accentColor);
        const soulRange = Math.max(...soulColor) - Math.min(...soulColor);
        const resolvedSoulColor = soulRange <= 8
            ? soulColor
            : canonicalSoulColor(soulColor);
        const soul = paletteForColor(resolvedSoulColor);

        for (let index = 0; index < output.length; index += 4) {
            const red = source[index];
            const green = source[index + 1];
            const blue = source[index + 2];
            if (source[index + 3] === 0) continue;

            const luminance = 0.2126 * red + 0.7152 * green + 0.0722 * blue;
            const isSoulPixel = blue >= 100
                && green >= 70
                && blue > red * 1.7
                && green > red * 1.7;
            const isGearPixel = red >= 20
                && blue >= red + 18
                && blue >= green + 8;
            if (!isSoulPixel && !isGearPixel) continue;

            const target = paletteTone(isSoulPixel ? soul : accent, luminance);
            output[index] = target[0];
            output[index + 1] = target[1];
            output[index + 2] = target[2];
        }
        return output;
    }

    function loadSource(source) {
        const absoluteSource = new URL(source, document.baseURI).href;
        if (!sourceCache.has(absoluteSource)) {
            const request = new Promise((resolve, reject) => {
                const image = new Image();
                image.decoding = 'async';
                image.onload = () => {
                    const canvas = document.createElement('canvas');
                    canvas.width = image.naturalWidth;
                    canvas.height = image.naturalHeight;
                    const context = canvas.getContext('2d', { willReadFrequently: true });
                    context.imageSmoothingEnabled = false;
                    context.drawImage(image, 0, 0);
                    const imageData = context.getImageData(0, 0, canvas.width, canvas.height);
                    resolve({
                        width: canvas.width,
                        height: canvas.height,
                        pixels: new Uint8ClampedArray(imageData.data)
                    });
                };
                image.onerror = () => reject(new Error(`Unable to load theme sprite: ${source}`));
                image.src = absoluteSource;
            }).catch(error => {
                sourceCache.delete(absoluteSource);
                throw error;
            });
            sourceCache.set(absoluteSource, request);
        }
        return sourceCache.get(absoluteSource);
    }

    async function renderSprite(source, color, mode) {
        const cacheKey = `${source}|${color.join(',')}|${mode}`;
        if (resultCache.has(cacheKey)) return resultCache.get(cacheKey);

        const original = await loadSource(source);
        const canvas = document.createElement('canvas');
        canvas.width = original.width;
        canvas.height = original.height;
        const context = canvas.getContext('2d');
        context.imageSmoothingEnabled = false;
        const spriteName = new URL(source, document.baseURI).pathname.split('/').pop();
        const recolored = recolorPixels(original.pixels, color, mode, spriteName);
        context.putImageData(new ImageData(recolored, original.width, original.height), 0, 0);
        const result = canvas.toDataURL('image/png');
        resultCache.set(cacheKey, result);
        return result;
    }

    async function renderAppIcon(accentColor, soulColor) {
        const accent = parseThemeColor(accentColor) || [205, 68, 81];
        const soul = parseThemeColor(soulColor) || [255, 0, 0];
        const cacheKey = `app-icon|${accent.join(',')}|${soul.join(',')}`;
        if (resultCache.has(cacheKey)) return resultCache.get(cacheKey);

        const original = await loadSource('./img/packIcon.png');
        const canvas = document.createElement('canvas');
        canvas.width = original.width;
        canvas.height = original.height;
        const context = canvas.getContext('2d');
        context.imageSmoothingEnabled = false;
        context.putImageData(new ImageData(
            recolorAppIconPixels(original.pixels, accent, soul),
            original.width,
            original.height
        ), 0, 0);
        const result = canvas.toDataURL('image/png');
        resultCache.set(cacheKey, result);
        return result;
    }

    async function applyAppIcon(theme, backend) {
        if (!backend?.invokeOptional) return false;
        const icon = await renderAppIcon(
            theme?.color || 'rgb(205, 68, 81)',
            theme?.soulColor || '#FF0000'
        );
        await backend.invokeOptional('setAppIcon', [icon], false);
        return true;
    }

    function identifySprite(image) {
        let source = image.dataset.themeSpriteSource;
        let mode = image.dataset.themeSprite;
        const currentSource = image.getAttribute('src') || '';

        if (!source && /(?:^|\/)sbar\/[^?#]+\.png$/i.test(currentSource)) {
            source = currentSource;
            mode = 'accent';
        } else if (!source && /(?:^|\/)img\/packIcon\.png$/i.test(currentSource)) {
            source = currentSource;
            mode = 'soul';
        }

        if (!source || !mode) return null;
        image.dataset.themeSpriteSource = source;
        image.dataset.themeSprite = mode;
        return { source, mode };
    }

    function findImages(root) {
        const images = [];
        if (root instanceof HTMLImageElement) images.push(root);
        if (root?.querySelectorAll) images.push(...root.querySelectorAll('img'));
        return images;
    }

    async function apply(themeColor, root = document) {
        const parsedColor = parseThemeColor(themeColor);
        if (!parsedColor) return;
        const color = canonicalSoulColor(parsedColor);

        const palette = color.join(',');
        if (palette !== activePalette) {
            activePalette = palette;
            resultCache.clear();
        }

        await Promise.all(findImages(root).map(async image => {
            const sprite = identifySprite(image);
            if (!sprite) return;
            const requestKey = `${sprite.source}|${palette}|${sprite.mode}`;
            image.dataset.themeSpriteRequest = requestKey;
            const result = await renderSprite(sprite.source, color, sprite.mode);
            if (image.dataset.themeSpriteRequest === requestKey && image.src !== result) {
                image.src = result;
            }
        }));
    }

    function observe(getThemeColor) {
        const observer = new MutationObserver(mutations => {
            const themeColor = getThemeColor();
            if (!themeColor) return;

            for (const mutation of mutations) {
                if (mutation.type === 'attributes') {
                    const source = mutation.target.getAttribute('src') || '';
                    if (!source.startsWith('data:')) {
                        delete mutation.target.dataset.themeSpriteSource;
                        delete mutation.target.dataset.themeSprite;
                        apply(themeColor, mutation.target).catch(() => {});
                    }
                    continue;
                }
                for (const node of mutation.addedNodes) {
                    if (node.nodeType === Node.ELEMENT_NODE) {
                        apply(themeColor, node).catch(() => {});
                    }
                }
            }
        });
        observer.observe(document.body, {
            childList: true,
            subtree: true,
            attributes: true,
            attributeFilter: ['src']
        });
        return () => observer.disconnect();
    }

    const api = Object.freeze({
        SOUL_COLORS,
        apply,
        canonicalSoulColor,
        controlPalette,
        observe,
        parseThemeColor,
        recolorPixels,
        recolorAppIconPixels,
        renderAppIcon,
        applyAppIcon
    });

    if (typeof module === 'object' && module.exports) {
        module.exports = api;
    } else {
        globalScope.ThemeSprites = api;
    }
})(typeof window === 'undefined' ? globalThis : window);
