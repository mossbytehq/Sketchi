const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const test = require('node:test');

const workflow = fs.readFileSync(
  path.join(__dirname, '../workflows/trusted-revision.yml'), 'utf8',
);
const script = workflow.replace(/\r\n/g, '\n').split('          script: |\n')[1]
  .split('\n').map(line => line.replace(/^ {12}/, '')).join('\n');
const validate = new (Object.getPrototypeOf(async function () {}).constructor)(
  'context', 'github', 'core', 'process', script,
);
const mainSha = 'a'.repeat(40);
const requestedSha = 'b'.repeat(40);

async function run({ status = 'ahead', ref = 'refs/heads/main', requested = '', fail = false } = {}) {
  const calls = [];
  const outputs = {};
  const github = { rest: { repos: {
    async getBranch(args) {
      calls.push(['branch', args]);
      if (fail) throw new Error('GitHub API unavailable');
      return { data: { commit: { sha: mainSha } } };
    },
    async getCommit(args) {
      calls.push(['commit', args]);
      return { data: { sha: requestedSha } };
    },
    async compareCommitsWithBasehead(args) {
      calls.push(['compare', args]);
      return { data: { status } };
    },
  } } };
  await validate(
    { ref, repo: { owner: 'mossbytehq', repo: 'Sketchi' } },
    github,
    { setOutput: (key, value) => { outputs[key] = value; } },
    { env: { REQUESTED_REF: requested } },
  );
  return { calls, outputs };
}

for (const status of ['ahead', 'identical']) {
  test(`accepts main history (${status}) and outputs only the resolved SHA`, async () => {
    const { calls, outputs } = await run({ status, requested: 'v1.2.3' });
    assert.equal(calls[0][1].branch, 'main');
    assert.equal(calls[1][1].ref, 'v1.2.3');
    assert.equal(calls[2][1].basehead, `${requestedSha}...${mainSha}`);
    assert.deepEqual(outputs, { sha: requestedSha });
  });
}

test('an omitted ref defaults to main', async () => {
  const { calls } = await run();
  assert.equal(calls[1][1].ref, 'refs/heads/main');
});

for (const status of ['behind', 'diverged', 'unexpected']) {
  test(`rejects an untrusted comparison (${status})`, async () => {
    await assert.rejects(run({ status }), /must be main or an ancestor/);
  });
}

test('rejects running the workflow from a feature branch even with a main input', async () => {
  await assert.rejects(
    run({ ref: 'refs/heads/feature', requested: 'main' }),
    /workflows must run from main/,
  );
});

test('fails closed on an API failure', async () => {
  await assert.rejects(run({ fail: true }), /GitHub API unavailable/);
});
