import { describe, it, expect, beforeAll, afterAll } from 'vitest';
import { startServices, stopServices, baseUrl } from './helpers/docker-lifecycle.js';
import { writeFileSync, unlinkSync, readFileSync } from 'fs';
import { join } from 'path';

describe('Feature: File Conversion Endpoint', () => {
  const testDocxPath = join('/tmp', 'test-e2e.docx');

  beforeAll(async () => {
    await startServices();

    // Create minimal valid DOCX (ZIP with word/document.xml)
    const JSZip = (await import('jszip')).default;
    const zip = new JSZip();
    zip.file('[Content_Types].xml', '<?xml version="1.0"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/></Types>');
    zip.file('word/document.xml', '<?xml version="1.0"?><w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t>Hello E2E Test</w:t></w:r></w:p></w:body></w:document>');
    zip.file('_rels/.rels', '<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>');

    const buffer = await zip.generateAsync({ type: 'nodebuffer' });
    writeFileSync(testDocxPath, buffer);
  }, 30000);

  afterAll(async () => {
    try { unlinkSync(testDocxPath); } catch {}
    await stopServices();
  }, 30000);

  it('Given a DOCX file When I POST to /api/convert Then I receive extracted text', async () => {
    const formData = new FormData();
    const fileBuffer = readFileSync(testDocxPath);
    const blob = new Blob([fileBuffer], {
      type: 'application/vnd.openxmlformats-officedocument.wordprocessingml.document'
    });
    formData.append('file', blob, 'test.docx');

    const response = await fetch(`${baseUrl()}/api/convert`, {
      method: 'POST',
      body: formData,
    });

    expect(response.status).toBe(200);

    const body = await response.json();
    expect(body.text).toContain('Hello E2E Test');
  });

  it('Given no file When I POST to /api/convert Then I receive 400', async () => {
    const boundary = '----test-boundary';
    const response = await fetch(`${baseUrl()}/api/convert`, {
      method: 'POST',
      headers: {
        'Content-Type': `multipart/form-data; boundary=${boundary}`,
      },
      body: `--${boundary}--\r\n`,
    });

    expect(response.status).toBe(400);
  });

  it('Given unsupported file type When I POST to /api/convert Then I receive 415', async () => {
    const formData = new FormData();
    const blob = new Blob(['plain text'], { type: 'text/plain' });
    formData.append('file', blob, 'test.txt');

    const response = await fetch(`${baseUrl()}/api/convert`, {
      method: 'POST',
      body: formData,
    });

    expect(response.status).toBe(415);
  });
});
