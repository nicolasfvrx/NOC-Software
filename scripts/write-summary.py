"""Render GitHub Actions results without treating missing builds as successful."""
import html
import json
import os
import sys
from pathlib import Path


def text(value):
    return html.escape(str(value)).replace('|', '&#124;').replace('\n', ' ')


def render(job_pages, artifact_pages, env):
    # Failed-job reruns can retain successful jobs from an earlier attempt.
    jobs = {}
    for page in job_pages:
        for job in page['jobs']:
            if job['id'] > jobs.get(job['name'], {}).get('id', -1):
                jobs[job['name']] = job
    artifacts = {
        item['name']: item for page in artifact_pages for item in page['artifacts']
        if not item.get('expired', False)
    }
    base = f"{env.get('GITHUB_SERVER_URL', 'https://github.com')}/{env['GITHUB_REPOSITORY']}"
    run = f"{base}/actions/runs/{env['GITHUB_RUN_ID']}"
    targets = []
    for app in ('noc-manager', 'noc-display'):
        for server in ('2012', '2016'):
            targets.append((f'{app} / Windows Server {server} x64',
                            f'{app}-windows-server-{server}-x64'))
    for ubuntu in ('22.04', '24.04'):
        targets.append((f'NOC Agent / ubuntu-{ubuntu} x64', f'noc-agent-ubuntu-{ubuntu}-x64'))
    labels = {'success': '✅ Réussi', 'failure': '❌ Échec', 'cancelled': '⏹ Annulé',
              'skipped': '⏭ Non exécuté', 'timed_out': '⏱ Délai dépassé'}

    def status(value):
        return labels.get(value, '❔ ' + text(value or 'Indisponible'))

    lines = ['# NOC — bilan des builds', '',
             f"Référence : **{text(env.get('GITHUB_REF_NAME', ''))}** · "
             f"Commit : `{text(env.get('GITHUB_SHA', '')[:12])}` · "
             f"Tentative : {text(env.get('GITHUB_RUN_ATTEMPT', '1'))}", '',
             '| Application / cible | Build, contrôles et paquet | Job complet | Archive |',
             '| --- | --- | --- | --- |']
    passed = 0
    uploaded = 0
    issues = []
    for name, artifact_name in targets:
        job = jobs.get(name)
        artifact = artifacts.get(artifact_name)
        build = next((s for s in (job or {}).get('steps', [])
                      if s['name'] == 'Build, verify and package'), None)
        build_result = (build or {}).get('conclusion')
        passed += build_result == 'success'
        job_result = (job or {}).get('conclusion')
        title = f"[{text(name)}]({job['html_url']})" if job else text(name)
        package = '— Aucune archive disponible'
        if artifact:
            uploaded += 1
            package = f"[Télécharger]({run}/artifacts/{artifact['id']}) ({artifact['size_in_bytes'] / 1048576:.1f} Mio)"
        lines.append(f'| {title} | {status(build_result)} | {status(job_result)} | {package} |')
        if not job:
            issues.append(f'**{text(name)}** : aucun job trouvé ; build non confirmé.')
        elif build_result != 'success' or job_result != 'success':
            failed = [s['name'] for s in job.get('steps', [])
                      if s.get('conclusion') in ('failure', 'cancelled', 'timed_out')]
            detail = ', '.join(map(text, failed)) or 'build non exécuté ou résultat indisponible'
            issues.append(f'**{text(name)}** : {detail}. [Voir les logs]({job["html_url"]}).')

    lines += ['', f'**{passed}/6 builds avec contrôles réussis · {uploaded}/6 archives disponibles.**', '',
              '## Échecs et éléments non construits', '']
    lines += ['- ' + issue for issue in issues] if issues else ['Tous les builds et envois d’archives ont réussi.']
    result = env.get('RELEASE_RESULT', 'skipped')
    eligible = env.get('GITHUB_EVENT_NAME') == 'push' and env.get('GITHUB_REF', '').startswith('refs/tags/v')
    lines += ['', '## Release', '']
    if result == 'success':
        lines.append(f'✅ [Release {text(env["GITHUB_REF_NAME"])} disponible]({base}/releases/tag/{env["GITHUB_REF_NAME"]}).')
    elif result == 'skipped' and not eligible:
        lines.append('⏭ Non prévue : cette exécution ne provient pas du push d’un tag de version `v…`.')
    elif result == 'skipped':
        lines.append('⏭ Publication bloquée : les builds requis n’ont pas tous réussi.')
    else:
        lines.append(f'{status(result)} : publication non confirmée. Un brouillon peut exister ; consulter le job de release.')
    lines += ['', 'Les archives Actions sont conservées 30 jours. Les liens de release donnent accès aux paquets publiés.',
              'Ces contrôles ne remplacent pas les essais graphiques et RDP sur les systèmes cibles.', '']
    return '\n'.join(lines)


if __name__ == '__main__':
    report = render(json.loads(Path(sys.argv[1]).read_text()),
                    json.loads(Path(sys.argv[2]).read_text()), os.environ)
    with open(os.environ['GITHUB_STEP_SUMMARY'], 'a', encoding='utf-8') as output:
        output.write(report)
