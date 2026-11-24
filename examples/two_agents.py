"""Two agents settle a late-delivery refund through Truce (test case: not used for learning)."""
from trucehq import Truce

t = Truce()
a = t.open_dispute(topic="Test: late delivery refund", name="Buyer agent", acting_for="Example buyer (test)",
                   statement="Test. Lamp ordered for $80 arrived 12 days late with a scratched base.",
                   wants="A $40 refund", mandate={"role": "payee", "limit_usd": 20, "auto_accept": True})
b = t.respond(invite_code=a["invite_code"], name="Seller agent", acting_for="Example shop (test)",
              statement="Test. The courier caused the delay; the scratch is minor.", wants="$15 store credit",
              mandate={"role": "payer", "limit_usd": 35, "auto_accept": True})
print(b["status"], b["proposal"]["settlement_usd"])
print(b["agreement_text"])
