// https://github.com/rodrigobastosv/fancy_password_field
import 'package:flutter/material.dart';
import 'package:flutter_hbb/common.dart';
import 'package:get/get.dart';
import 'package:password_strength/password_strength.dart';

abstract class ValidationRule {
  String get name;
  bool validate(String value);
}

class UppercaseValidationRule extends ValidationRule {
  @override
  String get name => translate('uppercase');
  @override
  bool validate(String value) {
    return value.runes.any((int rune) {
      var character = String.fromCharCode(rune);
      return character.toUpperCase() == character &&
          character.toLowerCase() != character;
    });
  }
}

class LowercaseValidationRule extends ValidationRule {
  @override
  String get name => translate('lowercase');

  @override
  bool validate(String value) {
    return value.runes.any((int rune) {
      var character = String.fromCharCode(rune);
      return character.toLowerCase() == character &&
          character.toUpperCase() != character;
    });
  }
}

class DigitValidationRule extends ValidationRule {
  @override
  String get name => translate('digit');

  @override
  bool validate(String value) {
    return value.contains(RegExp(r'[0-9]'));
  }
}

class SpecialCharacterValidationRule extends ValidationRule {
  @override
  String get name => translate('special character');

  @override
  bool validate(String value) {
    return value.contains(RegExp(r'[!@#$%^&*(),.?":{}|<>]'));
  }
}

class MinCharactersValidationRule extends ValidationRule {
  final int _numberOfCharacters;
  MinCharactersValidationRule(this._numberOfCharacters);

  @override
  String get name => translate('length>=$_numberOfCharacters');

  @override
  bool validate(String value) {
    return value.length >= _numberOfCharacters;
  }
}

class PasswordStrengthIndicator extends StatelessWidget {
  final RxString password;
  final double weakMedium = 0.33;
  final double mediumStrong = 0.67;
  const PasswordStrengthIndicator({Key? key, required this.password})
      : super(key: key);

  @override
  Widget build(BuildContext context) {
    return Obx(() {
      final value = password.value.trim();
      final strength = value.isEmpty ? 0.0 : estimatePasswordStrength(value);
      final fillColor =
          value.isEmpty ? const Color(0xFF6B7280) : _getColor(strength);
      final label = value.isEmpty ? '' : translate(_getLabel(strength));
      return Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          LayoutBuilder(
            builder: (context, constraints) {
              final width = constraints.maxWidth;
              final fillWidth = value.isEmpty
                  ? 0.0
                  : (width * strength.clamp(0.12, 1.0)).toDouble();
              return ClipRRect(
                borderRadius: BorderRadius.circular(999),
                child: Container(
                  height: 10,
                  color: Theme.of(context).dividerColor.withOpacity(0.16),
                  child: Align(
                    alignment: Alignment.centerLeft,
                    child: AnimatedContainer(
                      duration: const Duration(milliseconds: 180),
                      curve: Curves.easeOutCubic,
                      width: fillWidth,
                      decoration: BoxDecoration(
                        borderRadius: BorderRadius.circular(999),
                        gradient: LinearGradient(
                          colors: [
                            fillColor.withOpacity(0.72),
                            fillColor,
                          ],
                        ),
                        boxShadow: [
                          BoxShadow(
                            color: fillColor.withOpacity(0.22),
                            blurRadius: 10,
                            offset: const Offset(0, 2),
                          ),
                        ],
                      ),
                    ),
                  ),
                ),
              );
            },
          ),
          if (label.isNotEmpty)
            Container(
              margin: const EdgeInsets.only(top: 8),
              padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 3),
              decoration: BoxDecoration(
                color: fillColor.withOpacity(0.12),
                borderRadius: BorderRadius.circular(999),
              ),
              child: Text(
                label,
                style: TextStyle(
                  fontSize: 11,
                  fontWeight: FontWeight.w600,
                  color: fillColor,
                ),
              ),
            ),
        ],
      );
    });
  }

  String _getLabel(double strength) {
    if (strength < weakMedium) {
      return 'Weak';
    } else if (strength < mediumStrong) {
      return 'Medium';
    } else {
      return 'Strong';
    }
  }

  Color _getColor(double strength) {
    if (strength < weakMedium) {
      return const Color(0xFFE67E22);
    } else if (strength < mediumStrong) {
      return const Color(0xFF3B82F6);
    } else {
      return const Color(0xFF16A34A);
    }
  }
}
