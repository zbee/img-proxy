// IMG-Proxy Copyright (C) 2025 Ethan Henderson <ethan@zbee.codes>
// Licensed under GPLv3 - Refer to the LICENSE file for the complete text
// Find the source code at https://github.com/zbee/img-proxy

// Key for the current day+hour
const CURRENT_KEY = (() => {
    const now = new Date();
    const day = String(now.getDate()).padStart(2, '0');
    const hour = String(now.getHours()).padStart(2, '0');
    return `${day}-${hour}`;
})();
// Keys for the next 3 hours
const NEXT_KEYS = (() => {
    const keys = [];
    for (let i = 1; i < 4; i++) {
        const now = new Date();
        now.setHours(now.getHours() + i);
        const day = String(now.getDate()).padStart(2, '0');
        const hour = String(now.getHours()).padStart(2, '0');
        keys.push(`${day}-${hour}`);
    }
    return keys;
})();

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

async function storeAsset(env, response) {
    // Get the image data as an ArrayBuffer
    const imageData = await response.arrayBuffer();

    // Convert ArrayBuffer to base64
    const base64 = btoa(
        [...new Uint8Array(imageData)]
            .map(b => String.fromCharCode(b))
            .join('')
    );

    // Get content type of the original image
    const contentType = response.headers.get('content-type') || 'image/png';

    // Create data URL
    const dataUrl = `data:${contentType};base64,${base64}`;
    
    // Create array of current + next 3 hour keys
    const keys = [CURRENT_KEY, ...NEXT_KEYS];

    // Store the data URL in KV for each key
    const promises = keys.map(key => {
        return env.IMG_PROXY_CACHE.put(
            getDestinationKey(url.pathname) + "@" + key,
            dataUrl,
            { expirationTtl: WORKER_CACHE_TIME * 2, }
        );
    });

    // Wait for all KV storage operations to complete
    await Promise.all(promises);
}

async function serveAsset(request, env, context) {
    const url = new URL(request.url)
    const url_key = getDestinationKey(url.pathname)

    // Try to get the image from KV
    let value = await env.IMG_PROXY_CACHE.get(url_key + "@" + CURRENT_KEY)
    // Serve the image from KV, if it exists
    if (value != null) {
        return new Response(atob(value.split(',')[1]), {
            headers: { "content-type": value.split(';')[0].split(':')[1] }
        })
    }

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

    // Manually cache the image body
    storeAsset(env, response.clone())

    const headers = new Headers(response.headers)
    // Add caching header
    headers.set("cache-control", "public, max-age=" + CLIENT_CACHE_TIME)
    // Vary header so cache respects content-negotiation/auto-format
    headers.set("vary", "Accept")

    // Create response and add to the cache if successful
    response = new Response(response.body, { ...response, headers })

    return response
}

export default {
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