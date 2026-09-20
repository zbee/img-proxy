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
When a request is received, if it matches a stored image, then that
saved image will be served from Cloudflare KV.
If an image is due for a refresh, it attempts to re-fetch from the source,
falling back to the cached copy if the upstream is unreachable.\
The great part is that it will essentially always be cached.

Images have configurable refresh intervals (defaulting to 4 hours) or can be
uploaded statically, and are served with up to a 24 hour client-side cache.

All image data and metadata are stored in Cloudflare KV and managed via a
built-in web dashboard.

---

    zbee/IMG-Proxy: image url shortening, as well as caching and pre-caching.
    Copyright (C) 2026  Ethan Henderson (zbee) <ethan@zbee.codes>

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
