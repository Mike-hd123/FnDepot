package handlers

import (
	"net/http"

	"github.com/bestruirui/octopus/internal/relay"
	"github.com/bestruirui/octopus/internal/server/middleware"
	"github.com/bestruirui/octopus/internal/server/router"
	"github.com/looplj/axonhub/llm"
)

func init() {
	router.NewGroupRouter("/v1").
		Use(middleware.APIKeyAuth()).
		AddRoute(
			router.NewRoute("/chat/completions", http.MethodPost).
				Handle(relay.Forward(llm.APIFormatOpenAIChatCompletion)),
		).
		AddRoute(
			router.NewRoute("/responses", http.MethodPost).
				Handle(relay.Forward(llm.APIFormatOpenAIResponse)),
		).
		AddRoute(
			router.NewRoute("/messages", http.MethodPost).
				Handle(relay.Forward(llm.APIFormatAnthropicMessage)),
		)
}
