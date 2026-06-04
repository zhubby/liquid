.PHONY: frontend-dev ui-dev

frontend-dev:
	cd liquid-ui && bun run dev

ui-dev: frontend-dev

