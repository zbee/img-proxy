// IMG-Proxy Copyright (C) 2025 Ethan Henderson <ethan@zbee.codes>
// Licensed under GPLv3 - Refer to the LICENSE file for the complete text
// Find the source code at https://github.com/zbee/img-proxy

// Function to fetch the current UTC time from a public API
async function fetchCurrentUTC() {
    const response = await fetch('http://worldtimeapi.org/api/timezone/Etc/UTC');
    const data = await response.json();
    return new Date(data.utc_datetime);
}

// Key for the current day+hour
async function getCurrentKey() {
    const now = await fetchCurrentUTC();
    const day = String(now.getUTCDate()).padStart(2, '0');
    const hour = String(now.getUTCHours()).padStart(2, '0');
    return `${day}-${hour}`;
}

// Keys for the next 3 hours
async function getNextKeys() {
    const keys = [];
    const now = await fetchCurrentUTC();
    for (let i = 1; i < 4; i++) {
        const future = new Date(now);
        future.setUTCHours(future.getUTCHours() + i);
        const day = String(future.getUTCDate()).padStart(2, '0');
        const hour = String(future.getUTCHours()).padStart(2, '0');
        keys.push(`${day}-${hour}`);
    }
    return keys;
}

const WORKER_CACHE_TIME = 7200 // 2 hours
const CLIENT_CACHE_TIME = 172800 // 2 days
const CLIENT_LONG_CACHE_TIME = 31536000 // 1 year

// GitHub colors
const background = "24273a";
const text       = "cad3f5";
const icon       = "c6a0f6";
const accent     = "b7bdf8";

// Dictionary of destinations with shortnames as keys
const destinations = {
    // GH Stats headers
    "gh-overall-stats": `https://github-readme-stats-eight-weld-87.vercel.app/api?`
                        + `username=zbee&cache_seconds=${WORKER_CACHE_TIME}&`
                        + `custom_title=GitHub%20Stats&count_private=true&`
                        + `show_icons=true&line_height=24&hide_border=true&`
                        + `bg_color=${background}&text_color=${text}&`
                        + `icon_color=${icon}&title_color=${accent}`,
    "gh-language-stats": `https://github-readme-stats-eight-weld-87.vercel.app/api/top-langs/?`
                        + `username=zbee&cache_seconds=${WORKER_CACHE_TIME}&`
                        + `layout=compact&langs_count=8&hide_border=true&`
                        + `card_width=275&hide=hack,procfile,cmake&`
                        + `size_weight=0.6&count_weight=0.4&`
                         + `bg_color=${background}&text_color=${text}&`
                        + `icon_color=${icon}&title_color=${accent}`,
    // GH Badges
    "gh-badge-intellij": `https://img.shields.io/badge/ide-IntelliJ-informational?`
                        + `style=for-the-badge&logo=intellij-idea&`
                        + `cacheSeconds=${CLIENT_LONG_CACHE_TIME}&`
                        + `logoColor=${icon}&color=${accent}&`
                        + `labelColor=${background}`,
    "gh-badge-heroku": `https://img.shields.io/badge/cloud-Heroku-informational?`
                        + `style=for-the-badge&logo=heroku&`
                        + `cacheSeconds=${CLIENT_LONG_CACHE_TIME}&`
                        + `logoColor=${icon}&color=${accent}&`
                        + `labelColor=${background}`,
    "gh-badge-aws": `https://img.shields.io/badge/cloud-AWS-informational?`
                        + `style=for-the-badge&logo=amazonecs&`
                        + `cacheSeconds=${CLIENT_LONG_CACHE_TIME}&`
                        + `logoColor=${icon}&color=${accent}&`
                        + `labelColor=${background}`,
    "gh-badge-csharp": `https://img.shields.io/badge/lang-c%23-informational?`
                        + `style=for-the-badge&logo=.net&`
                        + `cacheSeconds=${CLIENT_LONG_CACHE_TIME}&`
                        + `logoColor=${icon}&color=${accent}&`
                        + `labelColor=${background}`,
    "gh-badge-python": `https://img.shields.io/badge/lang-python-informational?`
                        + `style=for-the-badge&logo=python&`
                        + `cacheSeconds=${CLIENT_LONG_CACHE_TIME}&`
                        + `logoColor=${icon}&color=${accent}&`
                        + `labelColor=${background}`,
    "gh-badge-cpp": `https://img.shields.io/badge/lang-c%2B%2B-informational?`
                        + `style=for-the-badge&logo=cplusplus&`
                        + `cacheSeconds=${CLIENT_LONG_CACHE_TIME}&`
                        + `logoColor=${icon}&color=${accent}&`
                        + `labelColor=${background}`,
    "gh-badge-php": `https://img.shields.io/badge/lang-php-informational?`
                        + `style=for-the-badge&logo=php&`
                        + `cacheSeconds=${CLIENT_LONG_CACHE_TIME}&`
                        + `logoColor=${icon}&color=${accent}&`
                        + `labelColor=${background}`,
};

// Function to get the destination key
function getDestinationKey(path) {
    return path.substring(path.lastIndexOf('/') + 1);
}

// Function to get the destination URL based on the path
function getDestination(path) {
    return destinations[getDestinationKey(path)] || null;
}

async function storeAsset(KV, bodyArray, contentType, url) {
    console.log('Starting storeAsset function');

    let imageData;

    // Get the image data as an ArrayBuffer
    imageData = bodyArray;
    console.log('Image data size:', imageData.byteLength, 'bytes');

    // Convert ArrayBuffer to base64
    const base64 = btoa(
        [...new Uint8Array(imageData)]
            .map(b => String.fromCharCode(b))
            .join('')
    );
    console.log('Base64 conversion complete, length:', base64.length);

    // Get content type of the original image
    contentType = contentType || "image/png";
    console.log('Content type:', contentType);

    // Create data URL
    const dataUrl = `data:${contentType};base64,${base64}`;
    console.log('Data URL created, starts with:', dataUrl.substring(0, 50) + '...');

    // Create array of current + next 3 hour keys
    const currentKey = await getCurrentKey();
    const nextKeys = await getNextKeys();
    const keys = [currentKey, ...nextKeys];
    console.log('Keys to store:', JSON.stringify(keys));

    // Store the data URL in KV for each key
    const promises = keys.map(key => {
        const fullKey = getDestinationKey(url.pathname) + "@" + key;
        console.log('Storing with key:', fullKey);
        return KV.put(
            fullKey,
            dataUrl,
            { expirationTtl: WORKER_CACHE_TIME * 2, }
        ).then(() => {
            console.log(`Successfully stored key: ${fullKey}`);
            return fullKey;
        }).catch(error => {
            console.error(`Failed to store key ${fullKey}:`, error);
            throw error;
        });
    });

    // Wait for all KV storage operations to complete
    try {
        const results = await Promise.all(promises);
        console.log('All keys stored successfully:', results);
    } catch (error) {
        console.error('Error storing keys:', error);
    }
}

async function serveAsset(request, env, context) {
    const url = new URL(request.url)
    const url_key = getDestinationKey(url.pathname)

    // Check that url_key is a valid destination
    if (!destinations[url_key]) {
        console.error('Invalid destination:', url_key);
        return new Response("Not Found", { status: 404 })
    }

    // Try to get the image from KV
    const currentKey = await getCurrentKey();
    let value = await env.IMG_PROXY_CACHE.get(url_key + "@" + currentKey)
    // Serve the image from KV, if it exists
    if (value != null) {
        console.log('Serving image from KV:', url_key + "@" + currentKey);
        return new Response(atob(value.split(',')[1]), {
            headers: { "content-type": value.split(';')[0].split(':')[1] }
        })
    }

    console.log('Image not found in KV, fetching from source:', url_key);

    // The desired URL
    let path = getDestination(url.pathname)

    // Request headers for content negotiation/auto-format, and caching
    let response = await fetch(
        path,
        {
            cf: {
                cacheTtlByStatus: { "200-299": WORKER_CACHE_TIME, "400-599": 0 },
                cacheEverything: true,
            },
            headers: request.headers,
        }
    )

    // Read the data for storage
    const arrayBuffer = await response.arrayBuffer();
    const contentType = response.headers.get("content-type");

    const headers = new Headers(response.headers)
    // Add caching header
    headers.set("cache-control", "public, max-age=" + CLIENT_CACHE_TIME)
    // Vary header so cache respects content-negotiation/auto-format
    headers.set("vary", "Accept")

    const finalResponse = new Response(arrayBuffer, { ...response, headers });

    // Manually cache the image body
    await storeAsset(env.IMG_PROXY_CACHE, arrayBuffer, contentType, url)

    return finalResponse;
}

export default {
    async scheduled(event, env, ctx) {
        // Fetch and store all assets on a schedule
        console.log('Running scheduled asset refresh');
        try {
            // Iterate through all destinations
            for (const [key, url] of Object.entries(destinations)) {
                console.log(`Fetching ${key} from ${url}`);
                
                try {
                    // Fetch the asset
                    const response = await fetch(url, {
                        cf: {
                            cacheTtlByStatus: { "200-299": WORKER_CACHE_TIME, "400-599": 0 },
                            cacheEverything: true,
                        }
                    });
                    
                    if (!response.ok) {
                        console.error(`Failed to fetch ${key}: ${response.status} ${response.statusText}`);
                        continue;
                    }
                    
                    // Get the content type and array buffer
                    const contentType = response.headers.get("content-type");
                    const arrayBuffer = await response.arrayBuffer();
                    
                    // Create a mock URL to pass to storeAsset
                    const mockUrl = new URL('https://example.com/' + key);
                    
                    // Store the asset in KV
                    await storeAsset(env.IMG_PROXY_CACHE, arrayBuffer, contentType, mockUrl);
                    console.log(`Successfully refreshed asset: ${key}`);
                } catch (error) {
                    console.error(`Error refreshing ${key}:`, error);
                }
            }
            console.log('Scheduled asset refresh complete');
        } catch (error) {
            console.error('Error in scheduled task:', error);
        }
    },
    async fetch(request, event, context) {
        // Get the response
        let response = await serveAsset(request, event, context)

        // If not a successful status code return response text
        if (!response || response.status > 399) {
            response = new Response(response.statusText, { status: response.status })
        }

        return response
    },
};