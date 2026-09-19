package httpx

import (
	"encoding/json"
	"net/http"
)

// WriteJSONError 统一返回 JSON 格式的错误响应
func WriteJSONError(w http.ResponseWriter, statusCode int, message string) {
	w.Header().Set("Content-Type", "application/json")
	w.WriteHeader(statusCode)
	json.NewEncoder(w).Encode(map[string]string{"status": "error", "message": message})
}

// RespondJSON 辅助函数：返回统一的 JSON 响应
func RespondJSON(w http.ResponseWriter, statusCode int, data interface{}) {
	w.Header().Set("Content-Type", "application/json")
	w.WriteHeader(statusCode)
	json.NewEncoder(w).Encode(data)
}
