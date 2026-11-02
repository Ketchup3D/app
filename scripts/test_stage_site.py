import json
from pathlib import Path
import tempfile
import unittest
import xml.etree.ElementTree as ET

from stage_site import DEFAULT_URL, FILES, stage


class StageSiteTests(unittest.TestCase):
    def test_discovery_files_and_application_urls_follow_pages_url(self):
        source = Path(__file__).resolve().parents[1] / 'site'
        for base in (DEFAULT_URL, 'https://example.org/', 'https://example.org/woodworking'):
            with self.subTest(base=base), tempfile.TemporaryDirectory() as directory:
                target = Path(directory)
                (target / 'play').mkdir()
                (target / 'play/index.html').write_text('existing editor')
                stage(source, target, base)
                url = base.rstrip('/') + '/'
                for name in FILES:
                    self.assertTrue((target / name).is_file())
                page = (target / 'index.html').read_text()
                data = json.loads(page.split('<script type="application/ld+json">')[1].split('</script>')[0])
                self.assertEqual(data['url'], url)
                self.assertEqual(data['offers']['price'], '0')
                self.assertIn(f'href="{url}"', page)
                locations = ET.parse(target / 'sitemap.xml').findall('{*}url/{*}loc')
                self.assertEqual([node.text for node in locations], [url])
                self.assertIn(url + 'sitemap.xml', (target / 'robots.txt').read_text())
                self.assertIn(f'({url})', (target / 'llms.txt').read_text())
                self.assertTrue((target / 'assets/screenshots/ketchup.png').is_file())
                self.assertEqual((target / 'play/index.html').read_text(), 'existing editor')
                if base != DEFAULT_URL:
                    for name in FILES:
                        self.assertNotIn(DEFAULT_URL, (target / name).read_text())

    def test_reject_invalid_public_urls(self):
        for url in ('http://example.org', 'https://', 'https://user:secret@example.org',
                    'https://example.org/?a=b', 'https://example.org/#fragment',
                    'https://example.org/\"invalid'):
            with self.subTest(url=url), tempfile.TemporaryDirectory() as directory:
                with self.assertRaises(ValueError):
                    stage(Path('site'), Path(directory), url)


if __name__ == '__main__':
    unittest.main()
