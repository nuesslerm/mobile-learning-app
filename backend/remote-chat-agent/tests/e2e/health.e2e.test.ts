import { describe, it, expect, beforeAll, afterAll } from 'vitest';
import { startServices, stopServices, baseUrl } from './helpers/docker-lifecycle.js';

describe('Feature: Health Check Endpoint', () => {
  beforeAll(async () => {
    await startServices();
  }, 30000);

  afterAll(async () => {
    await stopServices();
  }, 30000);

  it('Given the server is running When I GET /health Then I receive status ok', async () => {
    const response = await fetch(`${baseUrl()}/health`);

    expect(response.status).toBe(200);

    // The body also names the WebSocket protocol and the commit the service was built from.
    const body = await response.json();
    expect(body).toMatchObject({ status: 'ok', wsProtocolVersion: 1 });
    expect(typeof body.commit).toBe('string');
  });
});
