Our crawler fetches the same page twice because https://site.org/docs/ and https://site.org/docs count as different. Fix cache_key in urls.py so they are the same page.
