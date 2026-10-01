import { ResearchJob } from '../../src/research.ts';
import { JsonlAstraStore } from '../../src/store.ts';

const [root, jobId] = process.argv.slice(2);
const job = await ResearchJob.open(new JsonlAstraStore(root), jobId);
process.send({ ready: true });
process.once('message', async () => {
  try {
    await job.consumeTurns(1);
    process.send({ committed: true });
  } catch (error) {
    process.send({ committed: false, error: error.message });
  } finally { process.disconnect(); }
});
