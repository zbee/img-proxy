# My Personal Image proxy

> This proxy is made for my own usage, and as such is not designed to accept
> input from others, nor even necessarily work if deployed for your own use.

This Cloudflare worker is designed to consolidate all of my most commonly used
images (mostly those on my GitHub profile ReadMe), and provide them with a
variety of features:
- No more long load times when the dynamic images need rebuilt
- More consistent styling of dynamic images
- Much, much shorter URLs
- Matching load times, as they load from the exact same source
- Consistent caching

It does this with a pretty simple logic flow.\
When a request is received, if it matches one of the `destinations`, then that
saved image will be served.
If that image is already cached, it will be served from the cache, and
otherwise it will be served directly (and cached).\
The great part is that it will essentially always be cached.

The images are routinely fetched every ~3.5 hours and re-cached for 4 hours.\
Images are then served with a 24 hour client-side cache.

The caching itself is done in Cloudflare KV, to avoid as much difficulty as
possible in getting fresher images.

---

    zbee/IMG-Proxy: image url shortening, as well as caching and pre-caching.
    Copyright (C) 2025  Ethan Henderson (zbee) <ethan@zbee.codes>

     This program is free software: you can redistribute it and/or modify
     it under the terms of the GNU Affero General Public License as published
     by the Free Software Foundation, either version 3 of the License, or
     (at your option) any later version.

     This program is distributed in the hope that it will be useful,
     but WITHOUT ANY WARRANTY; without even the implied warranty of
     MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
     GNU Affero General Public License for more details.

     You should have received a copy of the GNU Affero General Public License
     along with this program. If not, see <https://www.gnu.org/licenses/>. 
