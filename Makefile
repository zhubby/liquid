UI_IMAGE ?= liquid-ui
UI_IMAGE_TAG ?= latest
UI_API_BASE_URL ?= http://localhost:3001

.PHONY: frontend-dev ui-dev frontend-image ui-image docs docs-build docs-serve

frontend-dev:
	cd liquid-ui && bun run dev

ui-dev: frontend-dev

frontend-image:
	docker build \
		--build-arg NEXT_PUBLIC_API_BASE_URL=$(UI_API_BASE_URL) \
		-t $(UI_IMAGE):$(UI_IMAGE_TAG) \
		./liquid-ui

ui-image: frontend-image

docs:
	mdbook build docs

docs-build: docs

docs-serve:
	mdbook serve docs
