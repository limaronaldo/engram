# Tasks: RML-928 Document Ingestion

## Preparação
- [ ] Adicionar dependências: `pdf_extract`, `pulldown-cmark`
- [ ] Definir tamanho default de chunk (1200 chars)
- [ ] Definir overlap default (200 chars)
- [ ] Definir limite máximo de arquivo (10MB)

## Implementação
- [ ] Criar `document_ingest.rs`
- [ ] Implementar parser Markdown (headings)
- [ ] Implementar parser PDF (texto por página)
- [ ] Implementar chunker com overlap
- [ ] Gerar hashes (file + chunk)
- [ ] Persistir chunks como memórias (tags + metadata)

## MCP
- [ ] Adicionar tool `memory_ingest_document` (path-only no MVP)
- [ ] Validar params + tamanho do arquivo
- [ ] Retornar resumo da ingestão

## Testes
- [ ] Teste MD simples
- [ ] Teste PDF simples
- [ ] Idempotência (re‑ingest)
- [ ] Oversize file

## Docs
- [ ] Atualizar AGENTS.md / CLAUDE.md
- [ ] Atualizar README
