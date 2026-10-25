#!/usr/bin/env python3
"""Real stdio/JSON-RPC integration tests. Only Python's standard library is used."""
import json
import os
from pathlib import Path
import queue
import subprocess
import tempfile
import threading
import time
import unittest

BINARY = Path(os.environ.get('VAMS_LSP_BIN', 'target/debug/vams-lsp')).resolve()


class RPC:
    def __init__(self, root, snippets=True, utf8=False):
        self.process = subprocess.Popen([str(BINARY)], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                        stderr=subprocess.PIPE)
        self.messages = queue.Queue()
        self.notifications = []
        self.counter = 0
        def reader():
            try:
                while True:
                    headers = {}
                    while True:
                        line = self.process.stdout.readline()
                        if not line:
                            return
                        if line in (b'\r\n', b'\n'):
                            break
                        key, value = line.decode().split(':', 1)
                        headers[key.lower()] = value.strip()
                    payload = self.process.stdout.read(int(headers['content-length']))
                    self.messages.put(json.loads(payload))
            except Exception as exc:
                self.messages.put(exc)
        self.thread = threading.Thread(target=reader, daemon=True)
        self.thread.start()
        capabilities = {'textDocument': {'completion': {'completionItem': {'snippetSupport': snippets}}}}
        if utf8:
            capabilities['general'] = {'positionEncodings': ['utf-8', 'utf-16']}
        self.initial = self.request('initialize', {'processId': os.getpid(), 'rootUri': root.as_uri(),
                                  'capabilities': capabilities})
        self.notify('initialized', {})

    def send(self, payload):
        if payload.get('params', object()) is None:
            payload.pop('params')
        raw = json.dumps(payload, ensure_ascii=False).encode()
        self.process.stdin.write(f'Content-Length: {len(raw)}\r\n\r\n'.encode() + raw)
        self.process.stdin.flush()

    def notify(self, method, params):
        self.send({'jsonrpc': '2.0', 'method': method, 'params': params})

    def request(self, method, params=None):
        self.counter += 1
        ident = self.counter
        self.send({'jsonrpc': '2.0', 'id': ident, 'method': method, 'params': params})
        deadline = time.monotonic() + 8
        while time.monotonic() < deadline:
            item = self.messages.get(timeout=max(.01, deadline-time.monotonic()))
            if isinstance(item, Exception):
                raise item
            if item.get('id') == ident:
                if 'error' in item:
                    raise RuntimeError(item['error'])
                return item.get('result')
            self.notifications.append(item)
        raise TimeoutError(method)

    def ready(self, count=1):
        for _ in range(100):
            info = self.request('workspace/executeCommand', {'command': 'vams.showProjectInfo'})
            if info['generation'] >= 1 and not info['indexing'] and info['indexed_files'] >= count:
                return info
            time.sleep(.02)
        raise AssertionError('indexing did not finish')

    def open(self, path, text, version=1):
        self.notify('textDocument/didOpen', {'textDocument': {'uri': path.as_uri(), 'languageId': 'verilogams',
                                                          'version': version, 'text': text}})

    def close(self):
        try:
            self.request('shutdown')
            self.notify('exit', None)
            self.process.wait(timeout=5)
        finally:
            if self.process.poll() is None:
                self.process.kill()
            self.process.wait(timeout=5)
            self.process.stdin.close()
            self.process.stdout.close()
            self.process.stderr.close()


class ProtocolTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix='vams project ')
        self.root = Path(self.temp.name).resolve()
        (self.root / '.vams.json').write_text('{}')
        self.model = self.root / 'resistor.va'
        self.model.write_text('module resistor(p,n); inout p,n; electrical p,n; parameter real r=1k; endmodule\n')
        self.top = self.root / 'top.va'
        self.text = 'module top;\n  resi\nendmodule\n'
        self.top.write_text(self.text)
        self.rpc = RPC(self.root)
        self.rpc.ready(2)

    def tearDown(self):
        self.rpc.close()
        self.temp.cleanup()

    def test_completion_capabilities_unsaved_changes_and_navigation(self):
        caps = self.rpc.initial['capabilities']
        self.assertFalse(caps.get('documentFormattingProvider', False))
        self.assertEqual('utf-16', caps['positionEncoding'])
        self.rpc.open(self.top, self.text)
        request = {'textDocument': {'uri': self.top.as_uri()}, 'position': {'line': 1, 'character': 6}}
        items = self.rpc.request('textDocument/completion', request)
        item = next(item for item in items if item['label'] == 'resistor')
        self.assertIn('${1:u_resistor}', item['textEdit']['newText'])
        self.assertIn('.p(', item['textEdit']['newText'])
        self.assertEqual(2, item['insertTextFormat'])
        # The edit range replaces exactly the typed prefix, not its indentation.
        self.assertEqual({'line': 1, 'character': 2}, item['textEdit']['range']['start'])
        self.rpc.notify('textDocument/didChange', {'textDocument': {'uri': self.top.as_uri(), 'version': 2},
                         'contentChanges': [{'range': {'start': {'line': 1, 'character': 2},
                                                      'end': {'line': 1, 'character': 6}},
                                             'text': 'resistor u(.p(),.n());'}]})
        definitions = self.rpc.request('textDocument/definition', request)
        self.assertEqual(self.model.as_uri(), definitions[0]['uri'])
        self.assertIsNotNone(self.rpc.request('textDocument/hover', request))
        self.assertTrue(self.rpc.request('textDocument/documentSymbol', {'textDocument': request['textDocument']}))
        refs = self.rpc.request('textDocument/references', dict(request, context={'includeDeclaration': True}))
        self.assertEqual(2, len(refs))
        # Unsaved target declaration moves the definition; it must override disk.
        self.rpc.open(self.model, '// μ😀 comment\n' + self.model.read_text())
        definitions = self.rpc.request('textDocument/definition', request)
        self.assertEqual(1, definitions[0]['range']['start']['line'])
        self.assertTrue(any(s['name'] == 'resistor' for s in self.rpc.request('workspace/symbol', {'query': 'res'})))
        # Repairing input clears syntax errors for version 2; other sources are untouched.
        self.rpc.request('workspace/executeCommand', {'command': 'vams.showProjectInfo'})
        notifications = [n['params'] for n in self.rpc.notifications if n.get('method') == 'textDocument/publishDiagnostics']
        self.assertTrue(any(n['uri'] == self.top.as_uri() and n.get('version') == 2 and not n['diagnostics'] for n in notifications))

    def test_completion_in_module_without_endmodule(self):
        self.rpc.open(self.top, 'module test2;\nresist\n')
        request = {'textDocument': {'uri': self.top.as_uri()}, 'position': {'line': 1, 'character': 6}}
        items = self.rpc.request('textDocument/completion', request)
        item = next(item for item in items if item['label'] == 'resistor')
        self.assertIn('${1:u_resistor}', item['textEdit']['newText'])
        self.assertIn('.p(', item['textEdit']['newText'])
        self.assertIn('.n(', item['textEdit']['newText'])
        self.assertEqual({'line': 1, 'character': 0}, item['textEdit']['range']['start'])
        self.assertEqual({'line': 1, 'character': 6}, item['textEdit']['range']['end'])
        # The context-only probe never hides the actual incomplete source error.
        self.rpc.request('workspace/executeCommand', {'command': 'vams.showProjectInfo'})
        diagnostics = [n['params'] for n in self.rpc.notifications
                       if n.get('method') == 'textDocument/publishDiagnostics']
        self.assertTrue(any(n['uri'] == self.top.as_uri() and n['diagnostics'] for n in diagnostics))

    def test_specific_syntax_diagnostics_and_keyword_templates(self):
        self.rpc.open(self.top, 'module test\n electrical p,,n;\nendmodule\n')
        self.rpc.request('textDocument/documentSymbol', {'textDocument': {'uri': self.top.as_uri()}})
        reports = [n['params']['diagnostics'] for n in self.rpc.notifications
                   if n.get('method') == 'textDocument/publishDiagnostics'
                   and n['params']['uri'] == self.top.as_uri()]
        diagnostics = reports[-1]
        messages = {d['message'] for d in diagnostics}
        self.assertEqual({'Missing `;` after module header', 'Unexpected `,`'}, messages)
        header = next(d for d in diagnostics if d['message'].startswith('Missing'))
        self.assertEqual({'line': 0, 'character': 11}, header['range']['start'])
        self.rpc.notify('textDocument/didChange', {'textDocument': {'uri': self.top.as_uri(), 'version': 2},
                         'contentChanges': [{'text': 'mod'}]})
        items = self.rpc.request('textDocument/completion', {'textDocument': {'uri': self.top.as_uri()},
                                                          'position': {'line': 0, 'character': 3}})
        module = next(i for i in items if i['label'] == 'module')
        self.assertEqual(2, module['insertTextFormat'])
        self.assertIn('endmodule', module['textEdit']['newText'])
        self.assertIn('${1:module_name}', module['textEdit']['newText'])

    def test_common_analog_errors_and_versioned_repair(self):
        self.rpc.open(self.top, 'module top;\n electrical p\n analog begin\n V(p)=;\nendmodule\n')
        self.rpc.request('textDocument/documentSymbol', {'textDocument': {'uri': self.top.as_uri()}})
        reports = [n['params'] for n in self.rpc.notifications if n.get('method') == 'textDocument/publishDiagnostics'
                   and n['params']['uri'] == self.top.as_uri()]
        messages = [d['message'] for d in reports[-1]['diagnostics']]
        for expected in ['Missing `;` after declaration', 'Use `<+`', 'Missing expression after `=`', 'Missing `end`']:
            self.assertTrue(any(expected in message for message in messages), messages)
        self.assertEqual(1, reports[-1]['version'])
        self.rpc.notify('textDocument/didChange', {'textDocument': {'uri': self.top.as_uri(), 'version': 2},
                         'contentChanges': [{'text': 'module top; electrical p; analog begin V(p)<+1; end endmodule'}]})
        self.rpc.request('workspace/executeCommand', {'command': 'vams.showProjectInfo'})
        reports = [n['params'] for n in self.rpc.notifications if n.get('method') == 'textDocument/publishDiagnostics'
                   and n['params']['uri'] == self.top.as_uri() and n['params'].get('version') == 2]
        self.assertTrue(any(not n['diagnostics'] for n in reports), reports)

    def test_refresh_file_changes_invalid_config_and_eof(self):
        extra = self.root / 'extra.va'
        extra.write_text('module extra; endmodule\n')
        info = self.rpc.request('workspace/executeCommand', {'command': 'vams.reindex'})
        self.assertEqual(3, info['indexed_files'])
        extra.unlink()
        self.assertEqual(2, self.rpc.request('workspace/executeCommand', {'command': 'vams.reindex'})['indexed_files'])
        (self.root / '.vams.json').write_text('{"unknown":true}')
        info = self.rpc.request('workspace/executeCommand', {'command': 'vams.reindex'})
        self.assertIn('unknown', info['configuration_error'])

    def test_plain_text_fallback_and_utf8_negotiation(self):
        other = RPC(self.root, snippets=False, utf8=True)
        try:
            other.ready(2)
            self.assertEqual('utf-8', other.initial['capabilities']['positionEncoding'])
            other.open(self.top, self.text)
            result = other.request('textDocument/completion', {'textDocument': {'uri': self.top.as_uri()},
                                                             'position': {'line': 1, 'character': 6}})
            item = next(item for item in result if item['label'] == 'resistor')
            self.assertNotIn('${', item['textEdit']['newText'])
        finally:
            other.close()


if __name__ == '__main__':
    unittest.main(verbosity=2)
