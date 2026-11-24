from dataclasses import dataclass
from html import escape
from http.server import BaseHTTPRequestHandler, HTTPServer
import json
import os
from pathlib import Path
from string import Template
import subprocess
import time
from typing import Callable
from urllib.parse import urlsplit


SITE = Path(__file__).resolve().parent
STATE = Path(os.environ.get('PAIR_STATE', SITE.parent))
HOST = os.environ['PAIR_HOST']
OWNER = os.environ['PAIR_OWNER']
PORT = 18798
ORIGIN = f'https://{HOST}:{PORT}'
T3_ADDRESS = f'https://{HOST}:18797'
ASSET_TYPES = {'.css': 'text/css; charset=utf-8', '.jpg': 'image/jpeg'}


@dataclass(frozen=True)
class PairingApp:
    name: str
    mark: str
    description: str
    create_offer: Callable[[], str]


def render(page, **values):
    return Template((SITE / 'pages' / f'{page}.html').read_text(encoding='utf-8')).substitute(values)


def offer(*args):
    return json.loads(subprocess.check_output(args, stderr=subprocess.DEVNULL, timeout=5))


def orca_offer():
    result = offer(
        STATE / 'bin/orca-serve', 'pair', '--state-dir', STATE / 'orca',
        '--endpoint', f'wss://{HOST}:16768',
    )
    return render('orca-result', link=escape(result['url']))


def t3_offer():
    result = offer(
        STATE / 'bin/t3-serve', 't3', 'pair', '--state-dir', STATE / 't3',
        '--base-url', T3_ADDRESS, '--label', 'iPhone',
    )
    return render('t3-result', address=escape(T3_ADDRESS), code=escape(result['code']))


APPS = {
    '/orca': PairingApp('Orca Mobile', 'O', 'Pair with a link that opens directly in Orca.', orca_offer),
    '/t3': PairingApp('T3 Code', 'T3', 'Add your Mac as an environment with a one-time code.', t3_offer),
}

ASSETS = {
    f'/assets/{path.name}': (path, ASSET_TYPES[path.suffix])
    for path in (SITE / 'static').iterdir()
    if path.is_file() and path.suffix in ASSET_TYPES
}


def app_cards():
    return ''.join(
        f'<a class="app-card" href="{escape(route)}">'
        f'<span class="app-mark" aria-hidden="true">{escape(app.mark)}</span>'
        f'<span class="app-name">{escape(app.name)}</span>'
        f'<span class="app-description">{escape(app.description)}</span>'
        '<span class="app-action">Pair app <span aria-hidden="true">↗</span></span></a>'
        for route, app in APPS.items()
    )


class Page(BaseHTTPRequestHandler):
    def log_message(self, *_):
        pass

    def send(self, status, body, content_type):
        self.send_response(status)
        self.send_header('Content-Type', content_type)
        self.send_header('Content-Length', str(len(body)))
        self.send_header('Cache-Control', 'no-store')
        self.send_header('Referrer-Policy', 'same-origin')
        self.send_header('X-Content-Type-Options', 'nosniff')
        self.end_headers()
        self.wfile.write(body)

    def reply(self, status, title, content, *, landing=False):
        document = render(
            'layout', title=escape(title), content=content,
            page_class='landing' if landing else 'guide',
            navigation='' if landing else '<a class="nav-back" href="/">All apps</a>',
        )
        self.send(status, document.encode(), 'text/html; charset=utf-8')

    def allowed(self):
        if self.headers.get('Tailscale-User-Login') == OWNER:
            return True
        self.reply(403, 'Access required', '<h1>Connect with your Tailscale account</h1>')
        return False

    def do_GET(self):
        if not self.allowed():
            return
        route = urlsplit(self.path).path
        if route == '/':
            self.reply(200, 'Choose an app', render('home', cards=app_cards()), landing=True)
        elif route in APPS:
            self.reply(200, f'Pair {APPS[route].name}', render(route[1:]))
        elif route in ASSETS:
            path, content_type = ASSETS[route]
            self.send(200, path.read_bytes(), content_type)
        else:
            self.reply(404, 'Page not found', '<h1>Page not found</h1><a href="/">Choose an app</a>')

    def do_POST(self):
        if not self.allowed():
            return
        app = APPS.get(self.path)
        if self.headers.get('Origin') != ORIGIN or app is None:
            self.reply(403, 'Pairing request blocked', '<h1>Open the pairing page again</h1><a href="/">Choose an app</a>')
            return
        try:
            content = app.create_offer()
        except (KeyError, ValueError, OSError, subprocess.SubprocessError):
            self.reply(
                503, 'Pairing unavailable',
                f'<h1>Couldn’t start pairing</h1><a href="{escape(self.path)}">Back to {escape(app.name)} setup</a>',
            )
            return
        self.reply(200, f'Finish pairing {app.name}', content)


def main():
    deadline = time.monotonic() + 7200
    with HTTPServer(('127.0.0.1', PORT), Page) as server:
        server.timeout = 1
        while time.monotonic() < deadline:
            server.handle_request()


if __name__ == '__main__':
    main()
