.DEFAULT_GOAL := help
SARE ?= sare
ZAINDU ?= zaindu
.PHONY: help pages local build today today-refresh poll proposals health viewer check
help:
	@echo 'pages: rebuild page changes; local: pages + git; build: include remote sources'
	@echo 'today: collect briefing inputs offline; today-refresh: refresh then collect'
	@echo 'poll: watch GitHub/Linear; proposals: pending events; health: graph health'
	@echo 'viewer: open ikusi; check: strict fusion gate'
pages:
	$(SARE) build pages --root "$(CURDIR)"
local:
	$(SARE) build local --root "$(CURDIR)"
build:
	$(SARE) build --root "$(CURDIR)"
today:
	$(SARE) today --root "$(CURDIR)"
today-refresh:
	$(SARE) today --refresh --root "$(CURDIR)"
poll:
	$(ZAINDU) poll --root "$(CURDIR)"
proposals:
	$(ZAINDU) proposals --root "$(CURDIR)"
health:
	@cat graph/views/health.md
viewer:
	THINKING_OS_ROOT="$(CURDIR)" ikusi
check:
	$(SARE) fuse --strict --root "$(CURDIR)"
