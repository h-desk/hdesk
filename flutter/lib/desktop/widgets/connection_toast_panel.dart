/// Minimal connection toast panel (ToDesk-style).
/// Shows device info + disconnect button in bottom-right,
/// auto-collapses to the right after 3 seconds.
/// NO windowManager calls inside — window stays fixed size.
library;

import 'dart:async';
import 'package:flutter/material.dart';

import '../../common.dart';
import '../../models/server_model.dart';
import '../../models/platform_model.dart';

/// Auto-collapse delay.
const Duration kAutoCollapseDelay = Duration(seconds: 3);

class ConnectionToastPanel extends StatefulWidget {
  final Client client;
  const ConnectionToastPanel({Key? key, required this.client}) : super(key: key);

  @override
  State<ConnectionToastPanel> createState() => _ConnectionToastPanelState();
}

class _ConnectionToastPanelState extends State<ConnectionToastPanel>
    with SingleTickerProviderStateMixin {
  bool _expanded = true;
  Timer? _autoCollapseTimer;
  late AnimationController _animController;
  late Animation<Offset> _slideAnim;

  Client get client => widget.client;

  @override
  void initState() {
    super.initState();
    _animController = AnimationController(
      vsync: this,
      duration: const Duration(milliseconds: 300),
    );
    // Slide from Offset.zero (visible) to Offset(0.85, 0) (mostly off-screen to right)
    _slideAnim = Tween<Offset>(
      begin: Offset.zero,
      end: const Offset(0.85, 0),
    ).animate(CurvedAnimation(parent: _animController, curve: Curves.easeInOut));

    _startAutoCollapseTimer();
  }

  @override
  void dispose() {
    _autoCollapseTimer?.cancel();
    _animController.dispose();
    super.dispose();
  }

  void _startAutoCollapseTimer() {
    _autoCollapseTimer?.cancel();
    _autoCollapseTimer = Timer(kAutoCollapseDelay, () {
      if (mounted && _expanded) {
        _collapse();
      }
    });
  }

  void _collapse() {
    setState(() => _expanded = false);
    _animController.forward();
  }

  void _expand() {
    setState(() => _expanded = true);
    _animController.reverse();
    _startAutoCollapseTimer();
  }

  void _handleDisconnect() {
    bind.cmCloseConnection(connId: client.id);
  }

  @override
  Widget build(BuildContext context) {
    return SlideTransition(
      position: _slideAnim,
      child: GestureDetector(
        onTap: () {
          if (!_expanded) {
            _expand();
          } else {
            _startAutoCollapseTimer();
          }
        },
        child: Container(
          decoration: BoxDecoration(
            color: const Color(0xFF1E1E2E),
            borderRadius: BorderRadius.circular(12),
            boxShadow: [
              BoxShadow(
                color: Colors.black.withValues(alpha: 0.3),
                blurRadius: 8,
                offset: const Offset(0, 2),
              ),
            ],
          ),
          padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 10),
          child: Row(
            children: [
              // Avatar
              _buildAvatar(),
              const SizedBox(width: 10),
              // Device info
              Expanded(
                child: Column(
                  mainAxisAlignment: MainAxisAlignment.center,
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    Text(
                      client.name.isNotEmpty ? client.name : 'Device',
                      style: const TextStyle(
                        color: Colors.white,
                        fontSize: 13,
                        fontWeight: FontWeight.w600,
                      ),
                      maxLines: 1,
                      overflow: TextOverflow.ellipsis,
                    ),
                    const SizedBox(height: 2),
                    Text(
                      client.peerId,
                      style: TextStyle(
                        color: Colors.white.withValues(alpha: 0.6),
                        fontSize: 11,
                      ),
                      maxLines: 1,
                      overflow: TextOverflow.ellipsis,
                    ),
                  ],
                ),
              ),
              const SizedBox(width: 8),
              // Disconnect button
              _buildDisconnectButton(),
            ],
          ),
        ),
      ),
    );
  }

  Widget _buildAvatar() {
    return Container(
      width: 36,
      height: 36,
      decoration: BoxDecoration(
        color: str2color(client.name),
        borderRadius: BorderRadius.circular(8),
      ),
      alignment: Alignment.center,
      child: Text(
        client.name.isNotEmpty ? client.name[0].toUpperCase() : '?',
        style: const TextStyle(
          color: Colors.white,
          fontWeight: FontWeight.bold,
          fontSize: 16,
        ),
      ),
    );
  }

  Widget _buildDisconnectButton() {
    return Material(
      color: Colors.transparent,
      child: InkWell(
        onTap: _handleDisconnect,
        borderRadius: BorderRadius.circular(8),
        child: Container(
          padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 6),
          decoration: BoxDecoration(
            color: Colors.redAccent,
            borderRadius: BorderRadius.circular(8),
          ),
          child: const Text(
            '断开',
            style: TextStyle(
              color: Colors.white,
              fontSize: 12,
              fontWeight: FontWeight.w600,
            ),
          ),
        ),
      ),
    );
  }
}
