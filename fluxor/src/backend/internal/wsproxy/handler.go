package wsproxy

import (
	"context"
	"fluxor/internal/config"
	"github.com/gorilla/websocket"
	"log"
	"net"
	"net/http"
)

// WsProxyHandler 保持不变
func WsProxyHandler(targetPath string) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		conn, err := upgrader.Upgrade(w, r, nil)
		if err != nil {
			log.Printf("[WS] 升级失败 (路径 %s): %v", targetPath, err)
			return
		}
		defer conn.Close()

		config.Mu.RLock()
		secret := config.Current.PanelSecret
		config.Mu.RUnlock()

		dialer := &websocket.Dialer{
			NetDialContext: func(ctx context.Context, network, addr string) (net.Conn, error) {
				return net.Dial("unix", config.CoreSocket)
			},
			ReadBufferSize:  1024,
			WriteBufferSize: 1024,
		}
		header := http.Header{}
		if secret != "" {
			header.Set("Authorization", "Bearer "+secret)
		}
		path := targetPath
		if r.URL.RawQuery != "" {
			path += "?" + r.URL.RawQuery
		}
		coreConn, _, err := dialer.Dial("ws://localhost"+path, header)
		if err != nil {
			// 内核未运行或连接失败是预期情况，不记录日志
			return
		}
		defer coreConn.Close()

		errChan := make(chan error, 2)

		go func() {
			for {
				msgType, msg, err := coreConn.ReadMessage()
				if err != nil {
					errChan <- err
					return
				}
				if err := conn.WriteMessage(msgType, msg); err != nil {
					errChan <- err
					return
				}
			}
		}()

		go func() {
			for {
				msgType, msg, err := conn.ReadMessage()
				if err != nil {
					errChan <- err
					return
				}
				if err := coreConn.WriteMessage(msgType, msg); err != nil {
					errChan <- err
					return
				}
			}
		}()

		<-errChan
	}
}
