.DEFAULT_GOAL := help
SARE ?= sare
ZAINDU ?= zaindu
ARTALDE ?= artalde
.PHONY: help pages local build today today-refresh poll proposals health viewer check
.PHONY: artalde artalde-check artalde-snapshot artalde-inbox artalde-commands status events
help:
	@echo 'pages: rebuild page changes; local: pages + git; build: include remote sources'
	@echo 'today: collect briefing inputs offline; today-refresh: refresh then collect'
	@echo 'poll: watch GitHub/Linear; proposals: pending events; health: graph health'
	@echo 'viewer: open ikusi; check: strict fusion gate'
	@echo 'artalde opens the dashboard for this wiki'
	@echo 'artalde-check validates the UI config, artalde-snapshot prints session JSON'
	@echo 'artalde-inbox lists your PR inbox, artalde-commands lists UI commands'
	@echo 'status shows watcher state, events lists the last 20 recorded events'
artalde:
	@THINKING_OS_ROOT="$(CURDIR)" $(ARTALDE)
artalde-check:
	@THINKING_OS_ROOT="$(CURDIR)" $(ARTALDE) check
artalde-snapshot:
	@THINKING_OS_ROOT="$(CURDIR)" $(ARTALDE) snapshot
artalde-inbox:
	@THINKING_OS_ROOT="$(CURDIR)" $(ARTALDE) inbox
artalde-commands:
	@$(ARTALDE) commands
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
status:
	@$(ZAINDU) status --root "$(CURDIR)"
events:
	@$(ZAINDU) events --limit 20 --root "$(CURDIR)"
proposals:
	$(ZAINDU) proposals --root "$(CURDIR)"
health:
	@cat graph/views/health.md
viewer:
	THINKING_OS_ROOT="$(CURDIR)" ikusi
check:
	$(SARE) fuse --strict --root "$(CURDIR)"
