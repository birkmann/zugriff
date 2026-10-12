"""Collect repository deployment/build diagnostics without exposing credentials."""
import json
import os
from pathlib import Path
from urllib.request import Request, urlopen
from urllib.error import HTTPError

repo = os.environ['GITHUB_REPOSITORY']
token = os.environ['GH_TOKEN']
base = f'https://api.github.com/repos/{repo}'
output = Path('diagnostics')
output.mkdir(exist_ok=True)
def get(path, text=False):
    request = Request(base + path, headers={'Authorization': f'Bearer {token}', 'Accept': 'application/vnd.github+json', 'X-GitHub-Api-Version': '2022-11-28'})
    try:
        with urlopen(request, timeout=60) as response:
            data = response.read().decode('utf-8', errors='replace')
            return data if text else json.loads(data)
    except HTTPError as e:
        return {'http_status': e.code, 'body': e.read().decode(errors='replace')}

report = {'pages': get('/pages'), 'runs': []}
for run in get('/actions/runs?per_page=15').get('workflow_runs', []):
    if run['name'] == 'Diagnostics':
        continue
    item = {k: run.get(k) for k in ('id', 'name', 'status', 'conclusion', 'head_sha', 'html_url')}
    jobs = get(f"/actions/runs/{run['id']}/jobs").get('jobs', [])
    item['jobs'] = [{k: job.get(k) for k in ('id', 'name', 'status', 'conclusion', 'steps')} for job in jobs]
    for job in jobs:
        if job['conclusion'] == 'failure':
            logs = get(f"/actions/jobs/{job['id']}/logs", text=True)
            if isinstance(logs, str):
                (output / f"job-{job['id']}.log").write_text('\n'.join(logs.splitlines()[-220:]).replace(token, '[REDACTED]'))
    report['runs'].append(item)
(output / 'status.json').write_text(json.dumps(report, indent=2).replace(token, '[REDACTED]'))
print('Saved Pages status, workflow summaries and failed-job log tails.')
