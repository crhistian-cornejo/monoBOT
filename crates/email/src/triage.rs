//! What Echo does with a message, by rules and nothing else: the senders the user marked as interesting come first;
//! what failed the sender's checks, an automatic sender that talks like a scam, a notice of shipping and a newsletter
//! are held back; a receipt or an invoice is routine even when a robot sent it; the rest is routine. A held message is
//! never deleted and never answered: it waits in «En espera» until the user decides, and giving it priority teaches
//! Echo that sender. The text it reads is evidence, not orders.

use std::collections::HashSet;

use crate::model::Triage;
use crate::mime::Parsed;

/// Phrases that scams and phishing use (already folded: lowercase, no accents).
const SCAM: &[&str] = &[
    "verifica tu cuenta", "verifique su cuenta", "cuenta suspendida", "cuenta bloqueada", "confirma tu identidad", "actualiza tus datos bancarios", "has ganado", "felicidades, ganaste",
    "reclama tu premio", "haga clic aqui para", "tu cuenta sera cerrada", "verify your account", "account suspended", "confirm your identity", "you have won", "claim your prize",
    "unusual sign-in activity", "your password has expired", "gift card", "tarjeta de regalo",
];
/// Words of a notice of shipping or delivery.
const SHIPPING: &[&str] = &[
    "tu pedido", "pedido enviado", "ha sido enviado", "seguimiento del envio", "numero de seguimiento", "codigo de seguimiento", "tu paquete", "entrega programada", "en camino",
    "your order", "has shipped", "tracking number", "out for delivery", "delivered", "package",
];
/// Words of a receipt, an invoice or a payment: mail that matters even when a robot sent it.
const TRANSACTIONAL: &[&str] = &[
    "recibo", "factura", "comprobante", "pago recibido", "confirmacion de pago", "confirmacion de pedido", "receipt", "invoice", "payment received", "order confirmation",
];
/// Senders that are not people.
const AUTOMATIC: &[&str] = &["noreply", "no-reply", "donotreply", "do-not-reply", "notifications@", "notificaciones@", "mailer-daemon", "newsletter", "marketing@", "promo"];

/// Lowercase without the accents of Spanish, to compare words.
pub fn fold(text: &str) -> String {
    text.to_lowercase().chars().map(|c| match c { 'á' | 'à' | 'ä' => 'a', 'é' | 'è' | 'ë' => 'e', 'í' | 'ì' | 'ï' => 'i', 'ó' | 'ò' | 'ö' => 'o', 'ú' | 'ù' | 'ü' => 'u', 'ñ' => 'n', other => other }).collect()
}

/// The decision and, when it holds the message back, why in the user's words.
pub fn classify(mail: &Parsed, interesting: &HashSet<String>) -> (Triage, Option<String>) {
    if interesting.contains(&mail.from_address) {
        return (Triage::Priority, None);
    }
    if mail.auth_failed {
        return (Triage::Held, Some("el remitente no pasó la verificación (SPF, DKIM o DMARC)".into()));
    }
    let said = fold(&format!("{} {}", mail.subject, mail.text.chars().take(2_000).collect::<String>()));
    let bulk = mail.bulk || mail.unsubscribe;
    let automatic = bulk || AUTOMATIC.iter().any(|word| mail.from_address.contains(word));
    // A scam's words from a sender that hides being a mailing: held as a scam. A newsletter that declares itself and
    // says them is held as what it is; from a person they prove nothing (a forwarded warning says them too).
    if automatic && !bulk && SCAM.iter().any(|phrase| said.contains(phrase)) {
        return (Triage::Held, Some("parece un intento de estafa o suplantación".into()));
    }
    if automatic && TRANSACTIONAL.iter().any(|word| said.contains(word)) {
        return (Triage::Routine, None);
    }
    if automatic && SHIPPING.iter().any(|word| said.contains(word)) {
        return (Triage::Held, Some("aviso de envío o entrega".into()));
    }
    if mail.unsubscribe || mail.bulk {
        return (Triage::Held, Some("boletín o promoción".into()));
    }
    (Triage::Routine, None)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mail(from: &str, subject: &str, text: &str) -> Parsed {
        Parsed { from_address: from.into(), from_name: from.into(), subject: subject.into(), text: text.into(), ..Parsed::default() }
    }

    fn none() -> HashSet<String> {
        HashSet::new()
    }

    #[test]
    fn an_ordinary_message_from_a_person_is_routine() {
        assert_eq!(classify(&mail("lucia@ejemplo.test", "Cena del sábado", "¿Vienes?"), &none()), (Triage::Routine, None));
    }

    #[test]
    fn a_sender_the_owner_marked_always_wins() {
        let mut interesting = HashSet::new();
        interesting.insert("banco@ejemplo.test".to_string());
        let mut bad = mail("banco@ejemplo.test", "Verifica tu cuenta", "haz clic aquí");
        bad.auth_failed = true;
        assert_eq!(classify(&bad, &interesting), (Triage::Priority, None));
    }

    #[test]
    fn a_failed_check_holds_the_message() {
        let mut bad = mail("x@ejemplo.test", "Hola", "hola");
        bad.auth_failed = true;
        assert_eq!(classify(&bad, &none()).0, Triage::Held);
    }

    #[test]
    fn a_scam_is_held_with_its_reason() {
        let (triage, reason) = classify(&mail("noreply@ejemplo.test", "Cuenta suspendida", "Verifica tu cuenta ahora"), &none());
        assert_eq!(triage, Triage::Held);
        assert!(reason.unwrap().contains("estafa"));
    }

    #[test]
    fn the_words_of_a_scam_from_a_person_hold_nothing_and_in_a_newsletter_say_newsletter() {
        assert_eq!(classify(&mail("ana@ejemplo.test", "Cuidado", "Me llegó un «verifica tu cuenta»"), &none()).0, Triage::Routine);
        let mut promo = mail("ofertas@tienda.test", "Gana una gift card", "…");
        promo.unsubscribe = true;
        assert_eq!(classify(&promo, &none()), (Triage::Held, Some("boletín o promoción".into())));
    }

    #[test]
    fn a_receipt_from_a_robot_is_routine() {
        assert_eq!(classify(&mail("noreply@tienda.test", "Tu recibo de compra", "Gracias"), &none()).0, Triage::Routine);
        let mut invoice = mail("facturacion@empresa.test", "Factura de octubre", "…");
        invoice.unsubscribe = true;
        assert_eq!(classify(&invoice, &none()).0, Triage::Routine);
    }

    #[test]
    fn a_notice_of_shipping_from_a_robot_is_held_but_one_from_a_person_is_not() {
        assert_eq!(classify(&mail("noreply@tienda.test", "Tu pedido está en camino", ""), &none()).0, Triage::Held);
        assert_eq!(classify(&mail("ana@ejemplo.test", "Tu paquete llegó a casa", ""), &none()).0, Triage::Routine);
    }

    #[test]
    fn a_newsletter_is_held() {
        let mut news = mail("boletin@ejemplo.test", "Novedades de octubre", "…");
        news.unsubscribe = true;
        assert_eq!(classify(&news, &none()), (Triage::Held, Some("boletín o promoción".into())));
    }

    #[test]
    fn accents_and_capitals_do_not_hide_a_phrase() {
        assert_eq!(classify(&mail("noreply@b.test", "VERIFICA TU CUENTA", ""), &none()).0, Triage::Held);
        assert_eq!(fold("Ñandú ÁÉÍÓÚ"), "nandu aeiou");
    }
}
