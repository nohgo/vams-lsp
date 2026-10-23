#!/usr/bin/env python3
"""Reproducible synthetic smoke benchmark; not a proprietary-model benchmark."""
import json
from pathlib import Path
import statistics
import sys
import tempfile
import time

sys.path.insert(0, str(Path(__file__).resolve().parent.parent / 'tests'))
from protocol import RPC

with tempfile.TemporaryDirectory(prefix='vams-benchmark-') as temporary:
    root = Path(temporary).resolve()
    (root / '.vams.json').write_text('{}')
    for index in range(1000):
        (root / f'm{index:04}.va').write_text(f'module m{index:04}(p,n); inout p,n; electrical p,n; parameter real r=1k; endmodule\n')
    top = root / 'top.va'
    source = 'module top;\n  m099\nendmodule\n'
    top.write_text(source)
    start = time.perf_counter()
    rpc = RPC(root)
    try:
        info = rpc.ready(1001)
        indexing = time.perf_counter() - start
        rpc.open(top, source)
        params = {'textDocument': {'uri': top.as_uri()}, 'position': {'line': 1, 'character': 6}}
        samples = []
        for _ in range(30):
            start = time.perf_counter()
            items = rpc.request('textDocument/completion', params)
            samples.append((time.perf_counter()-start)*1000)
            assert any(item['label'] == 'm0999' for item in items)
        print(json.dumps({'files': info['indexed_files'], 'startup_and_index_seconds': round(indexing, 3),
                          'completion_median_ms': round(statistics.median(samples), 3),
                          'completion_p95_ms': round(sorted(samples)[int(len(samples)*.95)-1], 3)}, indent=2))
    finally:
        rpc.close()
