# Search and AI discovery

The landing page serves woodworking use cases and application facts as static HTML, with SoftwareApplication JSON-LD, canonical and social metadata. Keep these facts consistent with the app and `llms.txt` when capabilities change. `llms.txt` is an optional summary for tools that read it; it is not an indexing requirement or a ranking signal guaranteed by search providers.

The Pages workflow stages `index.html`, `styles.css`, `sitemap.xml`, `robots.txt`, and `llms.txt` using `scripts/stage_site.py`. All public URLs follow the configured Pages base URL, including custom domains. The sitemap lists the informational landing page; the interactive WebAssembly editor is linked from that page.

Crawlers only consult robots.txt at the origin root. On the project site at https://ketchup3d.github.io/app/, the staged /app/robots.txt is informational; effective crawl rules must be maintained at https://ketchup3d.github.io/robots.txt in the organization Pages repository. Do not block search crawlers there. With a custom domain served at its root, the staged file is effective. No special AI directive is required: the wildcard rule permits all crawlers without distinguishing search from training crawlers.

After publication, verify the canonical URL and discovery files on the live site. The site owner can verify ownership in Google Search Console and Bing Webmaster Tools, submit the sitemap URL, and request indexing of the landing page. These steps require the owner's accounts; repository changes do not submit the site automatically. Indexing, citations, and recommendations remain at each provider's discretion.

Validate staging without compiling the app:

```sh
python3 -m unittest discover -s scripts -p 'test_stage_site.py'
```
